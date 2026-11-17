<#
.SYNOPSIS
  Saves a picture of a running program's window, without bringing it to the
  front or touching the mouse and keyboard.

.DESCRIPTION
  Used to check release builds of Mikoshi, where tools/cdp.mjs cannot attach
  (release builds refuse the debugging port). The window may be covered by
  other windows; it must not be minimized. Picks the largest visible window
  of the process, or the one whose title matches -Title.

.EXAMPLE
  pwsh tools/window-shot.ps1 -Out target/shots/release.png
#>
param(
    [string]$ProcessName = 'mikoshi',
    [string]$Title,
    [Parameter(Mandatory)][string]$Out
)

Add-Type -AssemblyName System.Drawing
Add-Type -Namespace Native -Name Win -MemberDefinition @'
public delegate bool EnumProc(System.IntPtr hwnd, System.IntPtr lParam);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, System.IntPtr lParam);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr hwnd, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr hwnd);
[DllImport("user32.dll")] public static extern bool IsIconic(System.IntPtr hwnd);
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr hwnd, out RECT rect);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(System.IntPtr hwnd, System.Text.StringBuilder text, int max);
[DllImport("user32.dll")] public static extern bool PrintWindow(System.IntPtr hwnd, System.IntPtr hdc, uint flags);
public struct RECT { public int Left, Top, Right, Bottom; }
'@

[void][Native.Win]::SetProcessDPIAware()

$ids = @(Get-Process $ProcessName -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
if (-not $ids) { throw "Process '$ProcessName' is not running." }

$windows = New-Object System.Collections.Generic.List[object]
$callback = [Native.Win+EnumProc] {
    param($hwnd, $lParam)
    $processId = [uint32]0
    [void][Native.Win]::GetWindowThreadProcessId($hwnd, [ref]$processId)
    if ($ids -contains $processId -and [Native.Win]::IsWindowVisible($hwnd)) {
        $rect = New-Object Native.Win+RECT
        [void][Native.Win]::GetWindowRect($hwnd, [ref]$rect)
        $text = New-Object System.Text.StringBuilder 256
        [void][Native.Win]::GetWindowText($hwnd, $text, 256)
        $windows.Add([pscustomobject]@{
                Handle = $hwnd
                Title  = $text.ToString()
                Width  = $rect.Right - $rect.Left
                Height = $rect.Bottom - $rect.Top
            })
    }
    return $true
}
[void][Native.Win]::EnumWindows($callback, [IntPtr]::Zero)

$candidates = $windows | Where-Object { $_.Width -gt 50 -and $_.Height -gt 50 }
if ($Title) { $candidates = $candidates | Where-Object { $_.Title -like $Title } }
$window = $candidates | Sort-Object { $_.Width * $_.Height } -Descending | Select-Object -First 1
if (-not $window) { throw "No visible window found for process '$ProcessName'." }
if ([Native.Win]::IsIconic($window.Handle)) { throw 'The window is minimized; restore it first.' }

$bitmap = New-Object System.Drawing.Bitmap $window.Width, $window.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$hdc = $graphics.GetHdc()
# 2 = PW_RENDERFULLCONTENT: needed for windows drawn by the GPU, like WebView2
$ok = [Native.Win]::PrintWindow($window.Handle, $hdc, 2)
$graphics.ReleaseHdc($hdc)
$graphics.Dispose()
if (-not $ok) { $bitmap.Dispose(); throw 'PrintWindow failed.' }

$bitmap.Save([System.IO.Path]::GetFullPath($Out), [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()
"saved $Out ($($window.Width)x$($window.Height), title: '$($window.Title)')"
