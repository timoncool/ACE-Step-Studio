param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputDirectory
)

# Builds HOT-Step's vst-host at the commit the trainer is pinned to and stages
# it with its license for the studio's resources. The host runs VST3 plugins in
# a process of its own; it needs neither CUDA nor ggml.

$PSDefaultParameterValues['*:ErrorAction'] = 'Stop'
$ErrorActionPreference = 'Continue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$source = Get-Content -Raw (Join-Path $repoRoot 'engines\music-train-source.json') | ConvertFrom-Json
$buildRoot = if ($env:YUE_ENGINE_BUILD_ROOT) { $env:YUE_ENGINE_BUILD_ROOT } else { $env:TEMP }
$worktree = Join-Path $buildRoot "hotstep-$($source.commit.Substring(0, 8))"

function Get-VcVars64 {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { throw 'vswhere.exe was not found; install Visual Studio C++ Build Tools.' }
    $installationPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installationPath)) { throw 'No Visual Studio C++ build installation was found.' }
    $vcvars = Join-Path $installationPath.Trim() 'VC\Auxiliary\Build\vcvars64.bat'
    if (-not (Test-Path $vcvars)) { throw "vcvars64.bat is missing: $vcvars" }
    return $vcvars
}

if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) { throw 'Ninja is required on PATH.' }

if (-not (Test-Path (Join-Path $worktree '.git'))) {
    git clone $source.repository $worktree
    if ($LASTEXITCODE -ne 0) { throw 'Could not clone HOT-Step-CPP.' }
}
git -C $worktree fetch origin $source.commit
if ($LASTEXITCODE -ne 0) { throw "Could not fetch HOT-Step-CPP commit $($source.commit)." }
git -C $worktree checkout --detach $source.commit
if ($LASTEXITCODE -ne 0) { throw "Could not check out HOT-Step-CPP commit $($source.commit)." }
git -C $worktree submodule update --init --recursive engine/ggml engine/vendor/vst3sdk
if ($LASTEXITCODE -ne 0) { throw 'Could not initialise the HOT-Step submodules.' }

$buildDirectory = 'build-vst-host'
$parallelism = [Math]::Max(1, [Environment]::ProcessorCount)
$command = "call `"$(Get-VcVars64)`" >nul && cmake -S . -B `"$buildDirectory`" -G Ninja -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DGGML_CUDA=OFF -DGGML_CCACHE=OFF && cmake --build `"$buildDirectory`" --target vst-host --parallel $parallelism"
Push-Location (Join-Path $worktree $source.source_dir)
try { & cmd.exe /d /s /c $command | Out-Host } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'The VST host build failed.' }

$built = Join-Path $worktree "$($source.source_dir)\$buildDirectory\vst-host.exe"
if (-not (Test-Path $built)) { throw 'The build completed without vst-host.exe.' }

$output = [System.IO.Path]::GetFullPath($OutputDirectory)
if ($output -eq [System.IO.Path]::GetPathRoot($output)) { throw 'OutputDirectory must be a specific child directory, not a drive root.' }
if (Test-Path $output) { Remove-Item -Recurse -Force $output }
New-Item -ItemType Directory -Force -Path $output | Out-Null
Copy-Item $built (Join-Path $output 'vst-host.exe') -Force
Copy-Item (Join-Path $worktree "$($source.source_dir)\LICENSE") (Join-Path $output 'LICENSE-HOT-Step.txt') -Force
$sdkLicense = Join-Path $worktree "$($source.source_dir)\vendor\vst3sdk\LICENSE.txt"
if (Test-Path $sdkLicense) { Copy-Item $sdkLicense (Join-Path $output 'LICENSE-VST3-SDK.txt') -Force }

$stamp = [pscustomobject]@{ commit = $source.commit; runtime = 'vst-host.exe' }
[System.IO.File]::WriteAllText((Join-Path $output 'runtime.json'), ($stamp | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))
[pscustomobject]@{ output = $output; commit = $source.commit } | ConvertTo-Json -Compress
