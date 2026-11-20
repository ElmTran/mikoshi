<#
.SYNOPSIS
  Development aid: starts the debug build of Mikoshi on the test virtual
  desktop without disturbing whoever is working on the current one.

.DESCRIPTION
  The test desktop is found by its name (Task View > right-click > Rename),
  "Mikoshi 测试" unless -DesktopName says otherwise. Windows keeps desktop
  ids and names in the registry; no undocumented API is needed. Nothing is
  started when there is no such desktop, or when it is the one in front.

  What keeps the test window out of the way (docs/architecture.md,
  "不打扰桌面的测试"):
  - MIKOSHI_DEV_DESKTOP makes Mikoshi move its own windows to that desktop
    (only a program may move its own windows), never activate a window,
    and leave global hotkeys and auto-type off.
  - Mikoshi is started through Task Scheduler and a hidden script host, not
    from this shell. A program started by the foreground process (Windows
    Terminal, when that is what the user is typing in) may take the
    foreground, which would switch the user's screen to the test desktop.
  - WebView2 flags keep the page drawing while nobody looks at that desktop,
    and open the debugging port tools/cdp.mjs drives.

  The Vite dev server must already be running on 127.0.0.1:1420
  (`pnpm exec vite` in the background).

.EXAMPLE
  pwsh tools/run-off-desktop.ps1
  pwsh tools/run-off-desktop.ps1 -DesktopName "Another name"
  pwsh tools/run-off-desktop.ps1 -DryRun
  pwsh tools/run-off-desktop.ps1 -Desktop 0be3f803-fe19-44ff-99af-3dbcf861013b
#>
param(
    [string]$DesktopName = "Mikoshi $([char]0x6D4B)$([char]0x8BD5)",
    [string]$Desktop,
    [int]$Port = 9333,
    # only report which desktop would be used
    [switch]$DryRun,
    [string]$Exe = (Join-Path $PSScriptRoot '..\target\debug\mikoshi.exe')
)

$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops'
$props = Get-ItemProperty $key -ErrorAction SilentlyContinue
$current = if ($props.CurrentVirtualDesktop) { [Guid]::new([byte[]]$props.CurrentVirtualDesktop) }

if (-not $Desktop) {
    $ids = [byte[]]$props.VirtualDesktopIDs
    $found = for ($i = 0; $i -lt $ids.Length / 16; $i++) {
        $g = [Guid]::new([byte[]]$ids[($i * 16)..($i * 16 + 15)])
        $name = (Get-ItemProperty "$key\Desktops\{$g}" -ErrorAction SilentlyContinue).Name
        if ($name -eq $DesktopName) { $g }
    }
    if (@($found).Count -ne 1) {
        Write-Error "no single virtual desktop named '$DesktopName' (found $(@($found).Count)); nothing started"
        exit 1
    }
    $Desktop = "$found"
}
if ($current -and [Guid]$Desktop -eq $current) {
    Write-Error "the test desktop is the one in front right now; nothing started"
    exit 1
}

if ($DryRun) {
    "would start on desktop $Desktop (current: $current)"
    exit 0
}

$exe = (Resolve-Path $Exe).Path
$root = Split-Path (Split-Path (Split-Path $exe))  # target\debug\x.exe -> repository
$flags = "--remote-debugging-port=$Port --disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows"
$vbs = Join-Path $env:TEMP 'mikoshi-off-desktop.vbs'
@"
Set sh = CreateObject("WScript.Shell")
Set env = sh.Environment("PROCESS")
env("MIKOSHI_DEV_DESKTOP") = "$Desktop"
env("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") = "$flags"
sh.CurrentDirectory = "$root"
' 0: the debug build is a console program; keep its console hidden
sh.Run """$exe""", 0, False
"@ | Set-Content -Path $vbs -Encoding ASCII

$task = 'mikoshi-off-desktop'
schtasks /create /tn $task /tr "wscript.exe //B //Nologo `"$vbs`"" /sc once /st 23:59 /f | Out-Null
schtasks /run /tn $task | Out-Null
Start-Sleep -Milliseconds 1500
schtasks /delete /tn $task /f | Out-Null
"started $exe via Task Scheduler (desktop $Desktop, CDP port $Port)"
