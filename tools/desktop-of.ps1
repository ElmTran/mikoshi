<#
.SYNOPSIS
  Development aid: says which virtual desktop a program's windows are on,
  and whether that is the desktop being shown right now.

.EXAMPLE
  pwsh tools/desktop-of.ps1 -ProcessName mikoshi
  pwsh tools/desktop-of.ps1 -Terminal      # the window this shell runs in
#>
param(
    [string]$ProcessName,
    [switch]$Terminal
)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

[ComImport, Guid("a5cd92ff-29be-454c-8d04-d82879fb3f1b"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IVirtualDesktopManager {
    [PreserveSig] int IsWindowOnCurrentVirtualDesktop(IntPtr hwnd, out bool onCurrent);
    [PreserveSig] int GetWindowDesktopId(IntPtr hwnd, out Guid desktop);
    [PreserveSig] int MoveWindowToDesktop(IntPtr hwnd, ref Guid desktop);
}

public static class Vd {
    static readonly Guid Clsid = new Guid("aa509086-5ca9-4c25-8f95-589d3c07b48a");
    static IVirtualDesktopManager M() {
        return (IVirtualDesktopManager)Activator.CreateInstance(Type.GetTypeFromCLSID(Clsid));
    }
    public static string Describe(IntPtr hwnd) {
        var m = M();
        bool cur; Guid id;
        int a = m.IsWindowOnCurrentVirtualDesktop(hwnd, out cur);
        int b = m.GetWindowDesktopId(hwnd, out id);
        return string.Format("desktop={0} onCurrent={1} (hr {2:X}/{3:X})", id, cur, a, b);
    }
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
}
'@

function Show-Window([IntPtr]$h, [string]$what) {
    $sb = [System.Text.StringBuilder]::new(256)
    [void][Vd]::GetWindowText($h, $sb, 256)
    "{0} hwnd=0x{1:X} title='{2}' visible={3} {4}" -f $what, $h.ToInt64(), $sb, [Vd]::IsWindowVisible($h), [Vd]::Describe($h)
}

if ($Terminal) {
    # walk up from this shell to the first ancestor that owns a window
    $p = Get-CimInstance Win32_Process -Filter "ProcessId=$PID"
    while ($p) {
        $proc = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
        if ($proc -and $proc.MainWindowHandle -ne 0) {
            Show-Window $proc.MainWindowHandle "terminal ($($proc.ProcessName))"
            break
        }
        $p = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.ParentProcessId)"
    }
}

if ($ProcessName) {
    $pids = (Get-Process -Name $ProcessName -ErrorAction SilentlyContinue).Id
    $found = [System.Collections.Generic.List[IntPtr]]::new()
    $cb = [Vd+EnumProc]{
        param($h, $l)
        [uint32]$wp = 0
        [void][Vd]::GetWindowThreadProcessId($h, [ref]$wp)
        if ($pids -contains $wp -and [Vd]::IsWindowVisible($h)) { $found.Add($h) }
        $true
    }
    [void][Vd]::EnumWindows($cb, [IntPtr]::Zero)
    foreach ($h in $found) { Show-Window $h $ProcessName }
}

Show-Window ([Vd]::GetForegroundWindow()) 'foreground'
