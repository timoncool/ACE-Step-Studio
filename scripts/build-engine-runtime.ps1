param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputDirectory,
    [ValidateSet('auto', 'cuda', 'vulkan', 'all')]
    [string]$RuntimeBackend = 'auto',
    [ValidateSet('universal', 'native', 'sm_89')]
    [string]$CudaArchitecture = 'universal',
    # CUDA 13 dropped Maxwell, Pascal and Volta, so a universal build adds a
    # second ggml-cuda from a CUDA 12 toolkit: the redist archives unpacked
    # into one folder, or an installed toolkit.
    [string]$Cuda12Root = $env:CUDA_PATH_V12_9
)

$ErrorActionPreference = 'Stop'
# git, cmake and the compiler all report progress on stderr. With the output
# redirected to a log, PowerShell treats every one of those lines as a
# terminating error, so the build died on "Cloning into ...". Every native call
# below checks $LASTEXITCODE, which is the actual verdict.
$PSDefaultParameterValues['*:ErrorAction'] = 'Stop'
$ErrorActionPreference = 'Continue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$engineSource = Get-Content -Raw (Join-Path $repoRoot 'engines\engine-source.json') | ConvertFrom-Json
$server = $engineSource.server
$engineName = $engineSource.id
# Windows still refuses paths past 260 characters, and the engine's own build
# tree is deep. A full commit hash under %TEMP% used up the budget before cmake
# had written a single object, so the checkout gets a short home; set
# STUDIO_ENGINE_BUILD_ROOT to move it to a shorter drive root if even that is tight.
$engineBuildRoot = if ($env:STUDIO_ENGINE_BUILD_ROOT) { $env:STUDIO_ENGINE_BUILD_ROOT } else { $env:TEMP }
# One checkout for every pinned commit: moving it to a new commit leaves the
# build directory in place, so Ninja recompiles only what the commit changed
# instead of all of ggml and its CUDA kernels.
$engineWorktree = Join-Path $engineBuildRoot $engineSource.worktree

function Test-CudaToolchain {
    $nvidiaSmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
    $nvcc = Get-Command nvcc -ErrorAction SilentlyContinue
    if (-not $nvidiaSmi -or -not $nvcc) { return $false }
    & $nvidiaSmi.Source -L *> $null
    return $LASTEXITCODE -eq 0
}

function Assert-VulkanSdk {
    $sdk = $env:VULKAN_SDK
    $glslc = Get-Command glslc -ErrorAction SilentlyContinue
    if ([string]::IsNullOrWhiteSpace($sdk) -or -not (Test-Path (Join-Path $sdk 'Include\vulkan\vulkan.h')) -or -not (Test-Path (Join-Path $sdk 'Lib\vulkan-1.lib')) -or -not $glslc) {
        throw 'The selected $engineName Vulkan build requires a Vulkan SDK with headers, vulkan-1.lib, and glslc; a Vulkan runtime alone is insufficient.'
    }
}

function Resolve-RuntimeBackend {
    if ($RuntimeBackend -eq 'auto') {
        if (Test-CudaToolchain) { return 'cuda' }
        if (-not [string]::IsNullOrWhiteSpace($env:VULKAN_SDK)) {
            Assert-VulkanSdk
            return 'vulkan'
        }
        throw 'No supported native build toolchain was detected. Install CUDA for NVIDIA or a complete Vulkan SDK, then select -RuntimeBackend explicitly.'
    }
    if ($RuntimeBackend -eq 'cuda' -and -not (Test-CudaToolchain)) {
        throw 'The CUDA build requires both a working NVIDIA driver (nvidia-smi) and nvcc.'
    }
    if ($RuntimeBackend -eq 'vulkan') { Assert-VulkanSdk }
    if ($RuntimeBackend -eq 'all') {
        if (-not (Test-CudaToolchain)) { throw 'The all-backends build requires both a working NVIDIA driver (nvidia-smi) and nvcc.' }
        Assert-VulkanSdk
    }
    return $RuntimeBackend
}

function Get-VcVars64 {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        throw 'A Visual Studio C++ build installation is required for a custom CUDA architecture build (vswhere.exe was not found).'
    }
    $installationPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installationPath)) {
        throw 'Could not find a Visual Studio C++ build installation for the custom CUDA architecture build.'
    }
    $vcvars = Join-Path $installationPath.Trim() 'VC\Auxiliary\Build\vcvars64.bat'
    if (-not (Test-Path $vcvars)) { throw "Visual Studio vcvars64.bat is missing: $vcvars" }
    return $vcvars
}

function Assert-SpecificOutputDirectory {
    $fullPath = [System.IO.Path]::GetFullPath($OutputDirectory)
    if ($fullPath -eq [System.IO.Path]::GetPathRoot($fullPath)) {
        throw 'OutputDirectory must be a specific child directory, not a drive root.'
    }
    return $fullPath
}

function Sync-PinnedSource {
    if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
        throw 'CMake is required to build the pinned $engineName runtime. Install CMake and add it to PATH.'
    }
    if (-not (Test-Path (Join-Path $engineWorktree '.git'))) {
        git clone --recurse-submodules $engineSource.repository $engineWorktree
        if ($LASTEXITCODE -ne 0) { throw 'Could not clone the pinned $engineName source.' }
    }
    git -C $engineWorktree fetch --depth 1 origin $engineSource.commit
    if ($LASTEXITCODE -ne 0) { throw "Could not fetch $engineName commit $($engineSource.commit)." }
    git -C $engineWorktree checkout --detach $engineSource.commit
    if ($LASTEXITCODE -ne 0) { throw "Could not check out $engineName commit $($engineSource.commit)." }
    git -C $engineWorktree submodule update --init --recursive
    if ($LASTEXITCODE -ne 0) { throw 'Could not initialise $engineName submodules.' }
}

function Invoke-CustomCudaBuild {
    # The engine's own buildcuda.cmd leaves GGML_NATIVE on, and ggml then sets
    # CMAKE_CUDA_ARCHITECTURES to "native" - a binary that only runs on the card
    # it was compiled on. A release must run on other people's cards, so the
    # universal build turns GGML_NATIVE off and lets ggml apply its documented
    # spread: virtual 50/61/70/75/80, real 86/89, virtual 90, and Blackwell on
    # CUDA 12.8 and above.
    $settings = switch ($CudaArchitecture) {
        # A release must run on other people's machines, so both halves are
        # spelled out: GGML_NATIVE off keeps ggml's CPU kernels off this exact
        # processor's instruction set, and the architecture list covers the
        # cards - device code for the GTX 16 and RTX 20 through 50 series. A card
        # left to PTX needs a driver as new as this toolkit: an older one fails
        # the first kernel with "PTX was compiled with an unsupported toolchain".
        # Upstream's own script leaves both at "whatever this machine is", which
        # produces a binary only this machine can run.
        # The backends load at run time (GGML_BACKEND_DL), so the CUDA 12
        # backend of Invoke-Cuda12Build can take this one's place, and Vulkan
        # serves AMD and Intel cards and NVIDIA cards neither CUDA build runs.
        # The trailing 120-virtual follows NVIDIA's "Building for Maximum
        # Compatibility" rule: without PTX for the newest architecture there is
        # nothing to JIT from and the kernel launch simply fails. ggml rewrites
        # it to 120a-virtual on the way through - its Blackwell kernels use FP4
        # tensor core instructions that only exist in 12Xa - so this buys PTX
        # for Blackwell variants, not for whatever comes after them. ggml's own
        # comment puts that boundary at Rubin.
        'universal' { '-DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DGGML_VULKAN=ON "-DCMAKE_CUDA_ARCHITECTURES=75-real;80-real;86-real;89-real;90-real;120a-real;120-virtual"' }
        'native' { '-DCMAKE_CUDA_ARCHITECTURES=native' }
        'sm_89' { '-DCMAKE_CUDA_ARCHITECTURES=89' }
        default { throw "No custom CMake architecture is defined for '$CudaArchitecture'." }
    }
    if ($CudaArchitecture -eq 'universal') { Assert-VulkanSdk }
    $vcvars = Get-VcVars64
    $buildDirectoryName = "build-cuda-$CudaArchitecture"
    $parallelism = [Math]::Max(1, [Environment]::ProcessorCount)
    # The server and the tools engine-source.json names are shipped; the
    # rest of the engine's binaries never leave the build directory.
    #
    # ccache, when it is installed, is what turns a rebuild from twenty minutes
    # into one: ggml picks it up on its own through GGML_CCACHE, and the CUDA
    # kernels - which are almost all of the time here - are what it caches.
    $ccache = if (Get-Command ccache -ErrorAction SilentlyContinue) { '-DGGML_CCACHE=ON' } else { '-DGGML_CCACHE=OFF' }
    # Flash attention kernels for every quantisation, not only the few ggml
    # compiles by default. Without them a quant with no kernel falls back to the
    # general path, which is the slow one - and the studio now offers quants
    # down to Q3, exactly the ones left out. It costs build time, nothing else.
    $flashAttention = '-DGGML_CUDA_FA_ALL_QUANTS=ON'
    # Symbols, kept beside the binary rather than thrown away.
    #
    # An engine that dies leaves Windows an address and nothing else -
    # "Exception code 0xc0000005, fault offset 0x1791c" - and without a program
    # database that address cannot be turned into a line of code. It costs a
    # file next to the executable and no speed: the optimiser is untouched.
    $symbols = '-DCMAKE_MSVC_DEBUG_INFORMATION_FORMAT=ProgramDatabase -DCMAKE_EXE_LINKER_FLAGS=/DEBUG -DCMAKE_SHARED_LINKER_FLAGS=/DEBUG'
    # Ninja drives nvcc and cl directly, so the build does not depend on the
    # CUDA MSBuild integration being installed into this Visual Studio.
    if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) { throw 'Ninja is required on PATH.' }
    # VSLANG=1033: Ninja reads header dependencies from cl's /showIncludes, which
    # a localised Visual Studio prints in its own language; without it an edited
    # header would not rebuild anything.
    # With runtime-loaded backends nothing links against ggml-cuda or the CPU
    # variants, so naming the two executables alone would skip them: the
    # universal build builds the whole tree, as upstream's buildall does.
    $targets = if ($CudaArchitecture -eq 'universal') { '' } else { (@($server) + @($engineSource.tools) | ForEach-Object { "--target $_" }) -join ' ' }
    $command = "set `"VSLANG=1033`" && call `"$vcvars`" >nul && cmake -S . -B `"$buildDirectoryName`" -G Ninja -DCMAKE_BUILD_TYPE=Release -DGGML_CUDA=ON $ccache $flashAttention $symbols $settings && cmake --build `"$buildDirectoryName`" $targets --parallel $parallelism"
    Push-Location $engineWorktree
    # The compiler's own output must not become this function's return value:
    # PowerShell returns everything a function writes, and the build directory
    # name came back with several thousand lines of cmake in front of it.
    try { & cmd.exe /d /s /c $command | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "$engineName CUDA build for $CudaArchitecture failed." }
    return $buildDirectoryName
}

# The CUDA 12 backend: ggml-cuda alone, from the same source, for the cards
# CUDA 13 no longer targets and for drivers older than CUDA 13. Device code
# for every architecture, including Turing and newer for those old drivers.
function Invoke-Cuda12Build {
    $nvcc = Join-Path $Cuda12Root 'bin\nvcc.exe'
    if (-not (Test-Path $nvcc)) { throw "The CUDA 12 backend needs a CUDA 12 toolkit; nvcc.exe is missing under '$Cuda12Root'. Set -Cuda12Root or CUDA_PATH_V12_9." }
    $root = $Cuda12Root.Replace('\', '/')
    $buildDirectoryName = 'build-cuda12-universal'
    # -Wno-deprecated-gpu-targets silences the notice that CUDA 12 is the last
    # toolkit for Maxwell, Pascal and Volta.
    $cudaFlags = '-Wno-deprecated-gpu-targets'
    $flags = "-DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CUDA=ON -DGGML_CUDA_FA_ALL_QUANTS=ON `"-DCMAKE_CUDA_ARCHITECTURES=52-real;60-real;61-real;70-real;75-real;80-real;86-real;89-real;90-real;120a-real`" `"-DCMAKE_CUDA_COMPILER=$root/bin/nvcc.exe`" `"-DCUDAToolkit_ROOT=$root`" `"-DCMAKE_CUDA_FLAGS=$cudaFlags`""
    $ccache = if (Get-Command ccache -ErrorAction SilentlyContinue) { '-DGGML_CCACHE=ON' } else { '-DGGML_CCACHE=OFF' }
    $symbols = '-DCMAKE_MSVC_DEBUG_INFORMATION_FORMAT=ProgramDatabase -DCMAKE_EXE_LINKER_FLAGS=/DEBUG -DCMAKE_SHARED_LINKER_FLAGS=/DEBUG'
    $parallelism = [Math]::Max(1, [Environment]::ProcessorCount)
    $vcvars = Get-VcVars64
    # nvcc 12.9 knows MSVC up to 14.4x (Visual Studio 2022); its front end
    # crashes on the headers of 14.5x. Visual Studio 2026 installs the 2022
    # toolset beside its own as the component
    # Microsoft.VisualStudio.Component.VC.14.44.17.14.x86.x64.
    $toolsRoot = Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $vcvars))) 'Tools\MSVC'
    $toolset = Get-ChildItem -Path $toolsRoot -Directory | Where-Object { $_.Name -match '^14\.[34]\d\.' } | Sort-Object { [version]$_.Name } | Select-Object -Last 1
    if (-not $toolset) { throw "The CUDA 12 backend needs an MSVC 14.3x/14.4x toolset beside this Visual Studio; add the component Microsoft.VisualStudio.Component.VC.14.44.17.14.x86.x64." }
    $vcvarsVersion = ($toolset.Name -split '\.')[0..1] -join '.'
    # --fresh: the toolset is part of the configuration, and a cache from
    # another one keeps its compiler. Unchanged objects are not rebuilt.
    $command = "set `"VSLANG=1033`" && set `"CUDA_PATH=$Cuda12Root`" && call `"$vcvars`" -vcvars_ver=$vcvarsVersion >nul && cmake --fresh -S . -B `"$buildDirectoryName`" -G Ninja -DCMAKE_BUILD_TYPE=Release $ccache $symbols $flags && cmake --build `"$buildDirectoryName`" --target ggml-cuda --parallel $parallelism"
    Push-Location $engineWorktree
    try { & cmd.exe /d /s /c $command | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw '$engineName CUDA 12 backend build failed.' }
    $dll = Get-ChildItem -Path (Join-Path $engineWorktree $buildDirectoryName) -Recurse -Filter 'ggml-cuda.dll' -File | Select-Object -First 1
    if (-not $dll) { throw 'The CUDA 12 build completed without ggml-cuda.dll.' }
    return $dll.DirectoryName
}

function Invoke-RuntimeBuild {
    $resolvedBackend = Resolve-RuntimeBackend
    if ($CudaArchitecture -ne 'universal' -and $resolvedBackend -ne 'cuda') {
        throw "-CudaArchitecture $CudaArchitecture is only supported with the CUDA backend; resolved backend is '$resolvedBackend'."
    }
    if ($resolvedBackend -eq 'cuda') {
        return Invoke-CustomCudaBuild
    }

    $buildScriptName = switch ($resolvedBackend) {
        'cuda' { 'buildcuda.cmd' }
        'vulkan' { 'buildvulkan.cmd' }
        'all' { 'buildall.cmd' }
    }
    $buildScript = Join-Path $engineWorktree $buildScriptName
    if (-not (Test-Path $buildScript)) { throw "Pinned $engineName build script is missing: $buildScript" }
    Push-Location $engineWorktree
    try { & $buildScript | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "$engineName $resolvedBackend build failed." }
    return 'build'
}

Sync-PinnedSource
$buildDirectoryName = Invoke-RuntimeBuild
$shipsTwoCudaBuilds = $CudaArchitecture -eq 'universal' -and (Resolve-RuntimeBackend) -eq 'cuda'
$cuda12Directory = if ($shipsTwoCudaBuilds) { Invoke-Cuda12Build } else { $null }
$runtime = @(
    (Join-Path $engineWorktree "$buildDirectoryName\Release\$server.exe"),
    (Join-Path $engineWorktree "$buildDirectoryName\bin\$server.exe"),
    (Join-Path $engineWorktree "$buildDirectoryName\$server.exe"),
    (Join-Path $engineWorktree "$server.exe")
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $runtime) { throw "$engineName build completed without $server.exe." }

$resolvedOutputDirectory = Assert-SpecificOutputDirectory
New-Item -ItemType Directory -Force -Path $resolvedOutputDirectory | Out-Null
Copy-Item $runtime (Join-Path $resolvedOutputDirectory "$server.exe") -Force
foreach ($tool in @($engineSource.tools)) {
    $path = Join-Path (Split-Path -Parent $runtime) "$tool.exe"
    if (-not (Test-Path $path)) { throw "$engineName build completed without $tool.exe." }
    Copy-Item $path $resolvedOutputDirectory -Force
}
Get-ChildItem -Path (Split-Path -Parent $runtime) -Filter '*.dll' -File | Copy-Item -Destination $resolvedOutputDirectory -Force
# Each CUDA backend in a folder of its own, none beside the executable: the
# studio names the one the card and its driver run in STUDIO_CUDA_BACKEND. The
# single ggml-cpu.dll of the old static layout gives way to the CPU variants.
if ($shipsTwoCudaBuilds) {
    foreach ($build in @(@{ Folder = 'cuda13'; Source = (Split-Path -Parent $runtime) }, @{ Folder = 'cuda12'; Source = $cuda12Directory })) {
        $folder = Join-Path $resolvedOutputDirectory $build.Folder
        New-Item -ItemType Directory -Force -Path $folder | Out-Null
        $source = Join-Path $build.Source 'ggml-cuda.dll'
        if (-not (Test-Path $source)) { throw "ggml-cuda.dll is missing from the $($build.Folder) build." }
        Copy-Item $source $folder -Force
    }
    # The CUDA 12 backend imports the CUDA runtime as a DLL, where CUDA 13's
    # cudart.lib links it in: it goes beside the executable, where the loader
    # resolves the backend's imports, as NVIDIA's redistribution terms allow.
    $cudart = Join-Path $Cuda12Root 'bin\cudart64_12.dll'
    if (-not (Test-Path $cudart)) { throw "cudart64_12.dll is missing from $Cuda12Root." }
    Copy-Item $cudart $resolvedOutputDirectory -Force
    foreach ($stale in @('ggml-cuda.dll', 'ggml-cpu.dll')) {
        $path = Join-Path $resolvedOutputDirectory $stale
        if (Test-Path $path) { Remove-Item $path -Force }
    }
}
# The engine's own symbols travel with it, so a crash address on someone else's
# machine can be read here.
Get-ChildItem -Path (Split-Path -Parent $runtime) -Filter "$server.pdb" -File -ErrorAction SilentlyContinue |
    Copy-Item -Destination $resolvedOutputDirectory -Force
if (-not (Test-Path (Join-Path $resolvedOutputDirectory "$server.exe"))) { throw "$server.exe was not staged into the requested output directory." }

# The Visual C++ runtime the engine imports is not staged here: the studio
# checks for it on the user's machine and runs Microsoft's own redistributable
# installer when it is genuinely missing, which is how every other application
# that links against it behaves.

[pscustomobject]@{
    backend = Resolve-RuntimeBackend
    cuda_architecture = $CudaArchitecture
    cuda_builds = if ($shipsTwoCudaBuilds) { @('cuda13', 'cuda12') } else { @() }
    runtime = Join-Path $resolvedOutputDirectory "$server.exe"
} | ConvertTo-Json -Compress
