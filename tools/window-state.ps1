<#
.SYNOPSIS
  Lists the top-level windows of a running program: visible, minimized, size,
  position, title, and whether it is the foreground window.

.DESCRIPTION
  Read-only. Used when checking release builds of Mikoshi, where
  tools/cdp.mjs cannot attach.

.EXAMPLE
  pwsh tools/window-state.ps1
#>
param([string]$ProcessName = 'mikoshi')

Add-Type -Namespace NativeState -Name Win -MemberDefinition @'
public delegate bool EnumProc(System.IntPtr hwnd, System.IntPtr lParam);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, System.IntPtr lParam);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr hwnd, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr hwnd);
[DllImport("user32.dll")] public static extern bool IsIconic(System.IntPtr hwnd);
[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr hwnd, out RECT rect);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(System.IntPtr hwnd, System.Text.StringBuilder text, int max);
public struct RECT { public int Left, Top, Right, Bottom; }
'@

[void][NativeState.Win]::SetProcessDPIAware()
$ids = @(Get-Process $ProcessName -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
if (-not $ids) { "process '$ProcessName' is not running"; return }

$foreground = [NativeState.Win]::GetForegroundWindow()
$rows = New-Object System.Collections.Generic.List[object]
$callback = [NativeState.Win+EnumProc] {
    param($hwnd, $lParam)
    $processId = [uint32]0
    [void][NativeState.Win]::GetWindowThreadProcessId($hwnd, [ref]$processId)
    if ($ids -contains $processId) {
        $rect = New-Object NativeState.Win+RECT
        [void][NativeState.Win]::GetWindowRect($hwnd, [ref]$rect)
        $text = New-Object System.Text.StringBuilder 256
        [void][NativeState.Win]::GetWindowText($hwnd, $text, 256)
        $width = $rect.Right - $rect.Left
        $height = $rect.Bottom - $rect.Top
        # skip the tiny helper windows every Windows program has
        if ($width -gt 50 -and $height -gt 20) {
            $rows.Add([pscustomobject]@{
                    Title      = $text.ToString()
                    Visible    = [NativeState.Win]::IsWindowVisible($hwnd)
                    Minimized  = [NativeState.Win]::IsIconic($hwnd)
                    Foreground = ($hwnd -eq $foreground)
                    Size       = "${width}x${height}"
                })
        }
    }
    return $true
}
[void][NativeState.Win]::EnumWindows($callback, [IntPtr]::Zero)
$rows | Sort-Object Size -Descending | Format-Table -AutoSize | Out-String -Width 120
