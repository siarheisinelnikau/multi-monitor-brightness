$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
$previousSmoke = $env:MMB_UPDATER_SMOKE
try {
    cargo build --locked --features updater-smoke-test
    if ($LASTEXITCODE -ne 0) { throw 'Updater regression build failed.' }
    $env:MMB_UPDATER_SMOKE = '1'
    & '.\target\debug\multi-monitor-brightness.exe' --test-updater
    if ($LASTEXITCODE -ne 0) { throw 'Updater regression failed.' }
} finally {
    $env:MMB_UPDATER_SMOKE = $previousSmoke
    Pop-Location
}
