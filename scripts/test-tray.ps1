param([switch]$Startup)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
$previousReport = $env:MMB_TRAY_SMOKE_REPORT
try {
    cargo build --locked --features tray-smoke-test
    if ($LASTEXITCODE -ne 0) { throw 'Tray regression build failed.' }
    $env:MMB_TRAY_SMOKE_REPORT = Join-Path (Get-Location) ('target\tray-smoke-' + [guid]::NewGuid().ToString('N') + '.json')
    if ($Startup) { & '.\target\debug\multi-monitor-brightness.exe' --demo --startup }
    else { & '.\target\debug\multi-monitor-brightness.exe' --demo }
    if ($LASTEXITCODE -ne 0) { throw 'Tray regression failed.' }
    $report = Get-Content -LiteralPath $env:MMB_TRAY_SMOKE_REPORT -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Tray regression did not complete.' }
    Write-Output ('Tray regression passed (' + $report.steps.Count + ' checks).')
} finally {
    $env:MMB_TRAY_SMOKE_REPORT = $previousReport
    Pop-Location
}
