param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+([\-+][0-9A-Za-z.-]+)?$')]
    [string]$Version,
    [string]$ReleaseNotes = "",
    [ValidateSet('auto', 'cuda', 'vulkan', 'all')]
    [string]$RuntimeBackend = 'auto',
    [ValidateSet('nsis')]
    [string]$BundleTarget = 'nsis',
    # The CUDA 12 toolkit of the engine's second CUDA backend.
    [string]$Cuda12Root = $env:CUDA_PATH_V12_9
)

$ErrorActionPreference = 'Stop'

# Where the signing material is kept, said here rather than in a document
# nobody opens: this is the message a person sees when they try to cut a
# release without it.
$whereTheKeyIs = @'
The updater signing key is kept in two places:

  * GitHub repository secrets of timoncool/MiniMax-Music3-Studio -
    TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD and
    TAURI_UPDATER_PUBKEY. Copy them out with:
        gh secret list -R timoncool/MiniMax-Music3-Studio
    (values cannot be read back - use the local copy below, or the workflow).

  * On the release machine: %USERPROFILE%\.tauri\mm3-release.key, its .pub, and
    its password in mm3-release.key.password beside them. This script reads
    that file on its own, so a release needs no environment variable at all.
    A password that exists only in a GitHub secret cannot be read back, and
    losing it means losing automatic updates for everyone already running the
    studio - so it lives next to the key it opens.

Set them for a manual build:
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -Raw "$env:USERPROFILE\.tauri\mm3-release.key").Trim()
    $env:TAURI_UPDATER_PUBKEY      = (Get-Content -Raw "$env:USERPROFILE\.tauri\mm3-release.key.pub").Trim()
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<the password stored in the secret above>"

The public half must also match tauri.conf.json, or every update is rejected.
Lose the private key and automatic updates end for everyone already running the
studio: a new key means a new installer, installed by hand.
'@

# The key lives beside its password on the release machine, and this reads both
# rather than only the password - which is what the help above always claimed it
# did. Setting the variables by hand still wins, for CI, where the key comes
# from a repository secret and never touches the disk.
$keyFile = Join-Path $env:USERPROFILE '.tauri\mm3-release.key'
$publicKeyFile = "$keyFile.pub"
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and (Test-Path $keyFile)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -Raw $keyFile).Trim()
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBKEY) -and (Test-Path $publicKeyFile)) {
    $env:TAURI_UPDATER_PUBKEY = (Get-Content -Raw $publicKeyFile).Trim()
}

if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    throw "TAURI_SIGNING_PRIVATE_KEY must contain the updater private key.`n`n$whereTheKeyIs"
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBKEY)) {
    throw "TAURI_UPDATER_PUBKEY must contain the matching public key.`n`n$whereTheKeyIs"
}
# A password is only needed for an encrypted signing key, and this checks the
# key itself rather than demanding the variable unconditionally: Windows cannot
# hold an empty environment variable at all, so requiring it made a release with
# an unencrypted key impossible to build.
#
# Without this check an encrypted key does not fail - it hangs. Tauri asks for
# the password on stdin, the build runs with no console, and the whole release
# sits there in silence after the installers are already on disk. That happened,
# and it looked exactly like a slow build.
$passwordFile = Join-Path $env:USERPROFILE '.tauri\mm3-release.key.password'
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) -and (Test-Path $passwordFile)) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -Raw $passwordFile).Trim()
}
$decodedKey = try { [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:TAURI_SIGNING_PRIVATE_KEY)) } catch { $env:TAURI_SIGNING_PRIVATE_KEY }
if ($decodedKey -match 'encrypted secret key' -and [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) {
    throw "The signing key is encrypted and TAURI_SIGNING_PRIVATE_KEY_PASSWORD is empty. Tauri would wait for it on stdin and this build would hang.`n`n$whereTheKeyIs"
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
    # Release output must be built from tested native code.  The installer
    # deliberately contains only executables and UI assets; model weights stay
    # in the first-run model manager and are never downloaded at launch.
    # The studio is a single executable: the service is compiled into the
    # desktop binary, so there is no second program to build, ship or launch.
    # A native program writing to stderr is not a failure, but with the error
    # preference on Stop and the output redirected to a log, PowerShell turns
    # every cargo warning into a terminating error. Exit codes are the truth
    # here, so they are checked directly.
    $ErrorActionPreference = 'Continue'
    & (Join-Path $PSScriptRoot 'build-minimax-runtime.ps1') -OutputDirectory $engineResourceRoot -RuntimeBackend $RuntimeBackend -CudaArchitecture universal -Cuda12Root $Cuda12Root
    if ($LASTEXITCODE -ne 0) { throw "the engine runtime build failed with exit code $LASTEXITCODE" }
    # The VST host follows the trainer's HOT-Step commit; rebuilt only when that moves.
    $trainSource = Get-Content -Raw (Join-Path $repoRoot 'engines\music-train-source.json') | ConvertFrom-Json
    $vstStampPath = Join-Path $vstResourceRoot 'runtime.json'
    $vstStamp = if (Test-Path $vstStampPath) { Get-Content -Raw $vstStampPath | ConvertFrom-Json } else { $null }
    if (-not ($vstStamp -and $vstStamp.commit -eq $trainSource.commit -and (Test-Path (Join-Path $vstResourceRoot 'vst-host.exe')))) {
        & (Join-Path $PSScriptRoot 'build-vst-host.ps1') -OutputDirectory $vstResourceRoot
        if ($LASTEXITCODE -ne 0) { throw "the VST host build failed with exit code $LASTEXITCODE" }
    }
    # The engine is built first on purpose: one of these tests reads the staged
    # bundle's import tables and fails if it names a library that is neither
    # beside it nor downloaded on first start. Run the other way round it would
    # only ever inspect the previous build - which is how a release shipped
    # without cuBLAS and could not start on a machine without the CUDA Toolkit.
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed with exit code $LASTEXITCODE" }

    $config = Get-Content -Raw $templatePath
    $config = $config.Replace('__TAURI_UPDATER_PUBKEY__', $env:TAURI_UPDATER_PUBKEY)
    $config = $config.Replace('__RELEASE_VERSION__', $Version)
    Set-Content -Path $releaseConfigPath -Value $config -NoNewline

    Push-Location $desktopRoot
    try {
        if ($BundleTarget -eq 'all') {
            npm exec tauri build -- --config $releaseConfigPath
        }
        else {
            npm exec tauri build -- --config $releaseConfigPath --bundles $BundleTarget
        }
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }

    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
    $bundleRoot = Join-Path $tauriRoot 'target\release\bundle'
    # NSIS only. The MSI was built by WiX and installed per-machine into
    # Program Files, where the studio cannot write - so an MSI installation put
    # its models in the user profile on C: no matter which drive was chosen for
    # the program itself. The updater never used it either.
    Get-ChildItem -Recurse -File $bundleRoot -Include '*.exe','*.sig' |
        Copy-Item -Destination $releaseDir -Force

    $portableRoot = Join-Path $releaseDir "MiniMax-Music3-Studio-$Version-portable"
    New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null
    # The binary's name comes from tauri.conf.json, not from this script: it was
    # spelled out here as the crate name, so renaming the executable broke the
    # portable build instead of just renaming it.
    $binaryName = (Get-Content -Raw (Join-Path $tauriRoot 'tauri.conf.json') | ConvertFrom-Json).mainBinaryName
    if ([string]::IsNullOrWhiteSpace($binaryName)) { $binaryName = 'minimax-music3-studio-desktop' }
    Copy-Item (Join-Path $tauriRoot "target\release\$binaryName.exe") (Join-Path $portableRoot 'MiniMax-Music3-Studio.exe') -Force
    Copy-Item $engineResourceRoot (Join-Path $portableRoot 'resources\engine') -Recurse -Force
    Copy-Item $vstResourceRoot (Join-Path $portableRoot 'resources\vst-host') -Recurse -Force
    New-Item -ItemType File -Path (Join-Path $portableRoot 'portable.flag') -Force | Out-Null
    Compress-Archive -Path "$portableRoot\*" -DestinationPath (Join-Path $releaseDir "MiniMax-Music3-Studio-$Version-portable.zip") -Force

    # Tauri signs updater artifacts, but intentionally does not invent a
    # hosting-specific latest.json.  Build it from the actual NSIS artifact so
    # the in-app updater always receives a real URL and signature.
    # The bundle directory keeps every installer ever built here, so "the first
    # one" was whichever sorted first - and 1.3.0 sorts before 1.3.1. The
    # manifest then pointed the updater at the previous release.
    $nsisInstaller = Get-ChildItem -Recurse -File (Join-Path $bundleRoot 'nsis') -Filter "*$Version*-setup.exe" |
        Select-Object -First 1
    if (-not $nsisInstaller) {
        throw 'NSIS setup executable is missing. Build with -BundleTarget nsis or all; updater releases require NSIS.'
    }
    $signaturePath = "$($nsisInstaller.FullName).sig"
    if (-not (Test-Path $signaturePath)) {
        throw "Signed updater artifact is missing: $signaturePath"
    }
    $signature = (Get-Content -Raw $signaturePath).Trim()
    if ([string]::IsNullOrWhiteSpace($signature)) {
        throw "Updater signature is empty: $signaturePath"
    }
    $assetName = $nsisInstaller.Name -replace ' ', '.'
    if ($assetName -ne $nsisInstaller.Name) {
        Copy-Item $nsisInstaller.FullName (Join-Path $releaseDir $assetName) -Force
        Copy-Item $signaturePath (Join-Path $releaseDir "$assetName.sig") -Force
    }
    $latest = [ordered]@{
        version = $Version
        notes = $ReleaseNotes
        pub_date = (Get-Date).ToUniversalTime().ToString('o')
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{
                signature = $signature
                url = "https://github.com/timoncool/MiniMax-Music3-Studio/releases/download/v$Version/$assetName"
            }
        }
    }
    # Windows PowerShell has no utf8NoBOM encoding, and a byte-order mark in
    # front of the JSON is enough for the updater to reject the manifest.
    [System.IO.File]::WriteAllText(
        (Join-Path $releaseDir 'latest.json'),
        ($latest | ConvertTo-Json -Depth 8),
        (New-Object System.Text.UTF8Encoding($false))
    )
}
finally {
    Remove-Item -LiteralPath $releaseConfigPath -Force -ErrorAction SilentlyContinue
    Pop-Location
}
