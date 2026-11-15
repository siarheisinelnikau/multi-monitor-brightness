$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
foreach ($taskCrate in @('rust-auto-updator', 'rust-auto-updator-egui')) {
    $taskManifest = Join-Path $taskRoot "vendor/$taskCrate/Cargo.toml"
    if (-not (Test-Path -LiteralPath $taskManifest)) {
        throw "Bundled updater source is missing: $taskManifest"
    }
}
Write-Output 'Bundled updater sources are present; external provisioning is unnecessary.'
