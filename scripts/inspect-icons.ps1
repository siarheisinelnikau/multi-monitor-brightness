param([string]$Executable,[string]$OutputDirectory)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path -Parent $PSScriptRoot
if (-not $Executable) {
    $taskManifest=Get-Content -LiteralPath (Join-Path $taskRoot 'Cargo.toml') -Raw
    $taskVersion=[regex]::Match($taskManifest,'(?m)^version = "([^"]+)"').Groups[1].Value
    $Executable=Join-Path $taskRoot "dist/MultiMonitorBrightness-v$taskVersion.exe"
}
if (-not $OutputDirectory) {$OutputDirectory=Join-Path $taskRoot ('dist/icons-'+[guid]::NewGuid().ToString('N'))}
$taskOutput=[IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $taskOutput) {throw 'Choose a new icon review directory'}
New-Item -ItemType Directory -Path $taskOutput | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MultiMonitorBrightnessIconResource {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr LoadLibraryExW(string path,IntPtr file,uint flags);
 [DllImport("kernel32")] public static extern bool FreeLibrary(IntPtr module);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr FindResourceW(IntPtr module,IntPtr name,IntPtr type);
 [DllImport("kernel32")] public static extern IntPtr LoadResource(IntPtr module,IntPtr resource);
 [DllImport("kernel32")] public static extern IntPtr LockResource(IntPtr resource);
 [DllImport("kernel32")] public static extern uint SizeofResource(IntPtr module,IntPtr resource);
 [DllImport("user32",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr LoadImageW(IntPtr module,IntPtr name,uint type,int x,int y,uint flags);
 [DllImport("user32")] public static extern bool DestroyIcon(IntPtr icon);
}
'@
# Data/image resource mapping only: this never executes the target's entry point.
$taskModule=[MultiMonitorBrightnessIconResource]::LoadLibraryExW([IO.Path]::GetFullPath($Executable),[IntPtr]::Zero,0x22)
if ($taskModule -eq [IntPtr]::Zero) {throw 'Cannot map executable icon resources'}
try {
    $taskGroup=[MultiMonitorBrightnessIconResource]::FindResourceW($taskModule,[IntPtr]1,[IntPtr]14)
    if ($taskGroup -eq [IntPtr]::Zero) {throw 'Packaged icon group is missing'}
    $taskLength=[MultiMonitorBrightnessIconResource]::SizeofResource($taskModule,$taskGroup)
    $taskData=[MultiMonitorBrightnessIconResource]::LockResource([MultiMonitorBrightnessIconResource]::LoadResource($taskModule,$taskGroup))
    $taskBytes=New-Object byte[] $taskLength
    [Runtime.InteropServices.Marshal]::Copy($taskData,$taskBytes,0,$taskLength)
    $taskCount=[BitConverter]::ToUInt16($taskBytes,4)
    if ($taskLength -ne 6+14*$taskCount) {throw 'Invalid icon group directory'}
    $taskSizes=@(for ($taskIndex=0;$taskIndex -lt $taskCount;$taskIndex++) {
        $taskWidth=[int]$taskBytes[6+14*$taskIndex]
        $taskHeight=[int]$taskBytes[7+14*$taskIndex]
        if ($taskWidth -eq 0) {$taskWidth=256}
        if ($taskHeight -eq 0) {$taskHeight=256}
        if ($taskWidth -ne $taskHeight) {throw 'Non-square icon resource'}
        $taskWidth
    })
    foreach ($taskSize in @(16,20,24,32,48,64,128,256)) {
        if ($taskSize -notin $taskSizes) {throw "Missing native $taskSize pixel icon resource"}
        $taskHandle=[MultiMonitorBrightnessIconResource]::LoadImageW($taskModule,[IntPtr]1,1,$taskSize,$taskSize,0)
        if ($taskHandle -eq [IntPtr]::Zero) {throw "Cannot load packaged icon at $taskSize pixels"}
        try {
            $taskIcon=[Drawing.Icon]::FromHandle($taskHandle)
            $taskBitmap=$taskIcon.ToBitmap()
            try {
                $taskVisible=0
                for ($taskY=0;$taskY -lt $taskSize;$taskY++) {for ($taskX=0;$taskX -lt $taskSize;$taskX++) {
                    if ($taskBitmap.GetPixel($taskX,$taskY).A -gt 32) {$taskVisible++}
                }}
                if ($taskVisible -lt $taskSize*$taskSize/4) {throw "Blank or excessively padded icon at $taskSize pixels"}
                $taskBitmap.Save((Join-Path $taskOutput "icon-$taskSize.png"),[Drawing.Imaging.ImageFormat]::Png)
            } finally {$taskBitmap.Dispose();$taskIcon.Dispose()}
        } finally {[void][MultiMonitorBrightnessIconResource]::DestroyIcon($taskHandle)}
    }
} finally {[void][MultiMonitorBrightnessIconResource]::FreeLibrary($taskModule)}
Write-Output "Loaded and rendered packaged Windows icon resources without launching: $taskOutput"
