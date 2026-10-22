# Checks that every DLL the downloaded tools import (MIDI, trainer, stem separator, karaoke, assistant), the delay-loaded
# ones included, is beside the tool, in the engine's folder the studio puts on PATH, or supplied by Windows and the
# display driver. The engine bundle itself is checked by `cargo test` in build-release.ps1.
#   scripts/check-dll-imports.ps1 -DataRoot <studio data folder with the tools installed> [-EngineRoot <engine folder>]
param(
    [Parameter(Mandatory = $true)][string]$DataRoot,
    [string]$EngineRoot = (Join-Path $PSScriptRoot '..\desktop\src-tauri\resources\engine')
)
$ErrorActionPreference = 'Stop'
$env:ACE_DLL_DATA = (Resolve-Path $DataRoot).Path
$env:ACE_DLL_ENGINE = (Resolve-Path $EngineRoot).Path
Push-Location (Join-Path $PSScriptRoot '..')
try {
    cargo test -p music-server --lib engine_runtime::tests::downloaded_runtimes_can_be_loaded -- --ignored --nocapture
    if ($LASTEXITCODE -ne 0) { throw "DLL import check failed (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}
