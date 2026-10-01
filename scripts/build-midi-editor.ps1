# Builds the MIDI editor (signal, from the pinned commit of our fork) and puts
# it in app/public/midi-editor, where the interface opens it in a frame.
$PSDefaultParameterValues['*:ErrorAction'] = 'Stop'
$ErrorActionPreference = 'Continue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$source = Get-Content -Raw (Join-Path $repoRoot 'engines\midi-editor-source.json') | ConvertFrom-Json
$buildRoot = if ($env:STUDIO_ENGINE_BUILD_ROOT) { $env:STUDIO_ENGINE_BUILD_ROOT } else { $env:TEMP }
$worktree = Join-Path $buildRoot 'signal-midi-editor'
$target = Join-Path $repoRoot 'app\public\midi-editor'

function Invoke-Checked([string]$what, [scriptblock]$command) {
    & $command
    if ($LASTEXITCODE -ne 0) { throw "$what failed with exit code $LASTEXITCODE" }
}

if (-not (Test-Path (Join-Path $worktree '.git'))) {
    Invoke-Checked 'git clone' { git clone $source.repository $worktree }
}
Push-Location $worktree
try {
    Invoke-Checked 'git fetch' { git fetch origin $source.branch }
    Invoke-Checked 'git checkout' { git checkout --detach $source.commit }
    Invoke-Checked 'npm ci' { npm ci --no-audit --no-fund }
    Invoke-Checked 'packages build' { npx turbo build --filter="./packages/*" }
    Invoke-Checked 'editor build' { npm run build:studio -w app }
} finally {
    Pop-Location
}

# the new build goes over the old one, and the files only the old one had
# (its hashed bundles) go to the recycle bin
$built = Join-Path $worktree 'app\dist-studio'
New-Item -ItemType Directory -Force $target | Out-Null
Copy-Item -Path (Join-Path $built '*') -Destination $target -Recurse -Force
$fresh = Get-ChildItem -LiteralPath $built -Recurse -File | ForEach-Object { $_.FullName.Substring($built.Length) }
Add-Type -AssemblyName Microsoft.VisualBasic
Get-ChildItem -LiteralPath $target -Recurse -File |
    Where-Object { $fresh -notcontains $_.FullName.Substring($target.Length) } |
    ForEach-Object { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($_.FullName, 'OnlyErrorDialogs', 'SendToRecycleBin') }
Write-Host "[OK] MIDI editor $($source.commit.Substring(0, 8)) -> $target"
