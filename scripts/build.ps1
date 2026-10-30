$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    cargo build --release --locked --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $outputDirectory = Join-Path (Get-Location) 'dist'
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    $metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Could not read package version.' }
    $package = $metadata.packages | Where-Object name -eq 'multi-monitor-brightness'
    $assetName = 'MultiMonitorBrightness-v' + $package.version + '.exe'
    $executable = Join-Path $outputDirectory $assetName
    Copy-Item -LiteralPath 'target\x86_64-pc-windows-msvc\release\multi-monitor-brightness.exe' -Destination $executable -Force
    $checksum = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText((Join-Path $outputDirectory 'SHA256SUMS.txt'), "$checksum  $assetName`n", [System.Text.UTF8Encoding]::new($false))
    Get-Item -LiteralPath $executable | Select-Object FullName, Length
} finally {
    Pop-Location
}
