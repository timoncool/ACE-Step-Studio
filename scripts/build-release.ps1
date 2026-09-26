param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+([\-+][0-9A-Za-z.-]+)?$')]
    [string]$Version,
    [string]$ReleaseNotes = "",
    [ValidateSet('auto', 'cuda', 'vulkan', 'all')]
    [string]$RuntimeBackend = 'auto',
    # The CUDA 12 toolkit of the engine's second CUDA backend.
    [string]$Cuda12Root = $env:CUDA_PATH_V12_9
)

$ErrorActionPreference = 'Stop'

$whereTheKeyIs = @'
The updater signing key lives on the release machine:
  %USERPROFILE%\.tauri\ace-step-studio.key, its .pub, and its password in
  ace-step-studio.key.password beside them. This script reads all three.
Set TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD /
TAURI_UPDATER_PUBKEY to override (CI). The public half must match
desktop/src-tauri/tauri.conf.json, or every update is rejected. Losing the
private key ends automatic updates for everyone already running the studio.
'@

$keyFile = Join-Path $env:USERPROFILE '.tauri\ace-step-studio.key'
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and (Test-Path $keyFile)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -Raw $keyFile).Trim()
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBKEY) -and (Test-Path "$keyFile.pub")) {
    $env:TAURI_UPDATER_PUBKEY = (Get-Content -Raw "$keyFile.pub").Trim()
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) -and (Test-Path "$keyFile.password")) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -Raw "$keyFile.password").Trim()
}
foreach ($name in 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_UPDATER_PUBKEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD') {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
        # An encrypted key with no password does not fail, it hangs on stdin.
        throw "$name is empty.`n`n$whereTheKeyIs"
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $repoRoot 'desktop'
$tauriRoot = Join-Path $desktopRoot 'src-tauri'
$templatePath = Join-Path $tauriRoot 'tauri.release.conf.template.json'
$releaseConfigPath = Join-Path $tauriRoot 'tauri.release.conf.json'
$releaseDir = Join-Path $repoRoot "release\$Version"
$engineResourceRoot = Join-Path $tauriRoot 'resources\engine'
$vstResourceRoot = Join-Path $tauriRoot 'resources\vst-host'

Push-Location $repoRoot
try {
    # Native tools write progress to stderr; exit codes are the verdict.
    $ErrorActionPreference = 'Continue'

    # The engine's checkout and build directories are kept between releases,
    # so Ninja rebuilds only what the pinned commit changed.
    & (Join-Path $PSScriptRoot 'build-engine-runtime.ps1') -OutputDirectory $engineResourceRoot -RuntimeBackend $RuntimeBackend -CudaArchitecture universal -Cuda12Root $Cuda12Root
    if ($LASTEXITCODE -ne 0) { throw "the engine runtime build failed with exit code $LASTEXITCODE" }

    # The VST host follows the trainer's HOT-Step commit; rebuilt only when that moves.
    $trainSource = Get-Content -Raw (Join-Path $repoRoot 'engines\music-train-source.json') | ConvertFrom-Json
    $vstStampPath = Join-Path $vstResourceRoot 'runtime.json'
    $vstStamp = if (Test-Path $vstStampPath) { Get-Content -Raw $vstStampPath | ConvertFrom-Json } else { $null }
    if (-not ($vstStamp -and $vstStamp.commit -eq $trainSource.commit -and (Test-Path (Join-Path $vstResourceRoot 'vst-host.exe')))) {
        & (Join-Path $PSScriptRoot 'build-vst-host.ps1') -OutputDirectory $vstResourceRoot
        if ($LASTEXITCODE -ne 0) { throw "the VST host build failed with exit code $LASTEXITCODE" }
    }

    # After the engine: one test reads the staged bundle's import tables and
    # fails if it names a library neither shipped nor downloaded on first start.
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed with exit code $LASTEXITCODE" }
    npm --prefix (Join-Path $repoRoot 'app') run test
    if ($LASTEXITCODE -ne 0) { throw "the interface tests failed with exit code $LASTEXITCODE" }

    $config = Get-Content -Raw $templatePath
    $config = $config.Replace('__TAURI_UPDATER_PUBKEY__', $env:TAURI_UPDATER_PUBKEY)
    $config = $config.Replace('__RELEASE_VERSION__', $Version)
    [System.IO.File]::WriteAllText($releaseConfigPath, $config, (New-Object System.Text.UTF8Encoding($false)))

    Push-Location $desktopRoot
    try {
        npm exec tauri build -- --config $releaseConfigPath --bundles nsis
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }

    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
    $bundleRoot = Join-Path $tauriRoot 'target\release\bundle'
    $nsisInstaller = Get-ChildItem -Recurse -File (Join-Path $bundleRoot 'nsis') -Filter "*_$($Version)_x64-setup.exe" | Select-Object -First 1
    if (-not $nsisInstaller) { throw "the NSIS installer for $Version is missing" }
    $signaturePath = "$($nsisInstaller.FullName).sig"
    if (-not (Test-Path $signaturePath)) { throw "the updater signature is missing: $signaturePath" }
    $signature = (Get-Content -Raw $signaturePath).Trim()

    # GitHub replaces spaces in asset names with dots; the manifest must name
    # the asset as it will be served.
    $assetName = $nsisInstaller.Name -replace ' ', '.'
    Copy-Item $nsisInstaller.FullName (Join-Path $releaseDir $assetName) -Force
    Copy-Item $signaturePath (Join-Path $releaseDir "$assetName.sig") -Force

    # Portable: the same executable, the engine beside it, and the marker that
    # keeps every byte of data inside the folder.
    $binaryName = (Get-Content -Raw (Join-Path $tauriRoot 'tauri.conf.json') | ConvertFrom-Json).mainBinaryName
    $portableRoot = Join-Path $releaseDir "ACE-Step-Studio-$Version-portable"
    if (Test-Path $portableRoot) { Remove-Item -Recurse -Force $portableRoot }
    New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null
    Copy-Item (Join-Path $tauriRoot "target\release\$binaryName.exe") (Join-Path $portableRoot "$binaryName.exe") -Force
    New-Item -ItemType Directory -Force -Path (Join-Path $portableRoot 'resources') | Out-Null
    Copy-Item $engineResourceRoot (Join-Path $portableRoot 'resources\engine') -Recurse -Force
    Get-ChildItem (Join-Path $portableRoot 'resources\engine') -Filter '*.pdb' | Remove-Item -Force
    Copy-Item $vstResourceRoot (Join-Path $portableRoot 'resources\vst-host') -Recurse -Force
    New-Item -ItemType File -Path (Join-Path $portableRoot 'portable.flag') -Force | Out-Null
    $portableZip = Join-Path $releaseDir "ACE-Step-Studio-$Version-portable-windows-x64.zip"
    if (Test-Path $portableZip) { Remove-Item -Force $portableZip }
    Compress-Archive -Path "$portableRoot\*" -DestinationPath $portableZip -Force

    $latest = [ordered]@{
        version = $Version
        notes = $ReleaseNotes
        pub_date = (Get-Date).ToUniversalTime().ToString('o')
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{
                signature = $signature
                url = "https://github.com/timoncool/ACE-Step-Studio/releases/download/v$Version/$assetName"
            }
        }
    }
    # A byte-order mark in front of the JSON makes the updater reject it.
    [System.IO.File]::WriteAllText(
        (Join-Path $releaseDir 'latest.json'),
        ($latest | ConvertTo-Json -Depth 8),
        (New-Object System.Text.UTF8Encoding($false))
    )
    Get-ChildItem $releaseDir -File | Select-Object Name, Length | Format-Table | Out-Host
}
finally {
    Remove-Item -LiteralPath $releaseConfigPath -Force -ErrorAction SilentlyContinue
    Pop-Location
}
