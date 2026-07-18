param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
$previousTest = $env:MMB_RENDER_TEST
$previousTrace = $env:MMB_RENDER_TRACE
try {
    if (-not $NoBuild) {
        cargo build --release --locked --target x86_64-pc-windows-msvc --features render-smoke-test,ui-capture,tray-smoke-test
        if ($LASTEXITCODE -ne 0) { throw 'CPU renderer regression build failed.' }
    }
    $output = Join-Path (Get-Location) 'dist\render-tests'
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $executable = Join-Path (Get-Location) 'target\x86_64-pc-windows-msvc\release\multi-monitor-brightness.exe'
    foreach ($mode in @('settings', 'popup', 'hidden', 'slider')) {
        $env:MMB_RENDER_TEST = $mode
        $env:MMB_RENDER_TRACE = Join-Path $output ($mode + '.json')
        $arguments = @('--demo')
        if ($mode -eq 'settings') { $arguments += '--settings' }
        if ($mode -eq 'hidden') { $arguments += '--startup' }
        $probe = Start-Process -FilePath $executable -ArgumentList $arguments -WindowStyle Hidden -PassThru
        $null = $probe.Handle
        Start-Sleep -Seconds 3
        $probe.Refresh()
        if ($probe.HasExited) { throw "Renderer probe exited early: $mode" }
        $cpu = $probe.TotalProcessorTime.TotalSeconds
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        Start-Sleep -Seconds 3
        $probe.Refresh()
        $cpuPercent = 100 * ($probe.TotalProcessorTime.TotalSeconds - $cpu) / $watch.Elapsed.TotalSeconds
        $graphics = @($probe.Modules | Where-Object { $_.ModuleName -match '^(opengl32|d3d9|d3d10|d3d11|d3d12|dxgi)\.dll$' } | ForEach-Object ModuleName)
        if (-not $probe.WaitForExit(10000)) { throw "Renderer probe did not stop: $mode" }
        if ($probe.ExitCode -ne 0) { throw "Renderer probe failed: $mode" }
        $report = Get-Content -LiteralPath $env:MMB_RENDER_TRACE -Raw | ConvertFrom-Json
        if (-not $report.probe.passed) { throw "Renderer counters failed: $mode" }
        if ($graphics.Count -ne 0) { throw "Unexpected graphics modules: $graphics" }
        $report | Add-Member -NotePropertyName one_core_cpu_percent -NotePropertyValue $cpuPercent
        $report | Add-Member -NotePropertyName graphics_modules -NotePropertyValue $graphics
        $report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $env:MMB_RENDER_TRACE -Encoding utf8
        Write-Output ("PASS {0}: CPU {1:N2}% of one core; rasterized {2}; no OpenGL/DirectX modules" -f $mode, $cpuPercent, (($report.probe.views | Measure-Object -Property rasterized -Sum).Sum))
    }
} finally {
    if ($probe -and -not $probe.HasExited) { Stop-Process -Id $probe.Id -ErrorAction SilentlyContinue }
    $env:MMB_RENDER_TEST = $previousTest
    $env:MMB_RENDER_TRACE = $previousTrace
    Pop-Location
}
