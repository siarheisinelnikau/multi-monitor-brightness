$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    cargo build --release --locked --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $outputDirectory = Join-Path (Get-Location) 'dist'
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    $executable = Join-Path $outputDirectory 'MultiMonitorBrightness.exe'
    Copy-Item -LiteralPath 'target\x86_64-pc-windows-msvc\release\multi-monitor-brightness.exe' -Destination $executable -Force
    $checksum = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText((Join-Path $outputDirectory 'SHA256SUMS.txt'), "$checksum  MultiMonitorBrightness.exe`n", [System.Text.UTF8Encoding]::new($false))
    Get-Item -LiteralPath $executable | Select-Object FullName, Length
} finally {
    Pop-Location
}
