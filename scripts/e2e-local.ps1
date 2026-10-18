<#
.SYNOPSIS
  Live end-to-end test of Moush on a single Windows machine.

.DESCRIPTION
  Runs the real GUI app as host (real low-level hooks) and the headless CLI as a dry-run
  client over loopback, then drives the host with synthetic input:

    1. push the cursor against the left edge   -> input must switch to the client
    2. move, press F24, click, scroll          -> all must arrive at the client
    3. move back right                          -> input must return to the host

  Synthetic input is normally ignored by the hooks; the host is started with
  MOUSH_TEST_ACCEPT_INJECTED=1 so this script can drive it. The client only logs
  what it would inject, so nothing is actually typed or clicked. The test briefly moves
  your cursor and puts it back; don't touch the mouse while it runs (~10 s).

  Uses throw-away configuration in %TEMP%; your real Moush settings are untouched.
  Assumes the primary monitor sits at (0,0) with nothing to its left.

.EXAMPLE
  npx tauri build --no-bundle; cargo build --release -p moush-cli
  powershell -ExecutionPolicy Bypass -File scripts\e2e-local.ps1
#>
param(
  [string]$Bin = "$PSScriptRoot\..\target\release",
  [int]$Port = 24990
)
$ErrorActionPreference = "Stop"
$Bin = (Resolve-Path $Bin).Path
$work = Join-Path $env:TEMP "moush-e2e-$PID"
$hostCfg = "$work\roam\Moush"; $cliCfg = "$work\cli"
New-Item -ItemType Directory -Force $hostCfg, $cliCfg, "$work\local" | Out-Null

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class In {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int d, IntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  public struct POINT { public int X; public int Y; }
  public static void MoveAbs(int x, int y) {
    int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
    int nx = (int)Math.Min(65535, (((long)(x - vx) * 65536) + vw - 1) / vw);
    int ny = (int)Math.Min(65535, (((long)(y - vy) * 65536) + vh - 1) / vh);
    mouse_event(0x0001 | 0x8000 | 0x4000, nx, ny, 0, IntPtr.Zero);
  }
}
"@
[In]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
function Pos { $p = New-Object In+POINT; [In]::GetCursorPos([ref]$p) | Out-Null; $p }
function Identity($dir) {
  $ErrorActionPreference = "Continue"   # the CLI logs to stderr; that is not a failure
  $o = & "$Bin\moush.exe" identity --config-dir $dir 2>$null
  @{ id = ($o -match '^id')[0].Split(' ', [StringSplitOptions]::RemoveEmptyEntries)[1];
     key = ($o -match '^public key')[0].Split(' ', [StringSplitOptions]::RemoveEmptyEntries)[2] }
}

# Pair both sides in advance and place the client directly left of the primary monitor.
$h = Identity $hostCfg; $c = Identity $cliCfg
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
ConvertTo-Json @(@{ id = $c.id; publicKey = $c.key; name = "E2E Client"; role = "client"; pairedAt = $now }) | Set-Content "$hostCfg\trusted.json"
ConvertTo-Json @(@{ id = $h.id; publicKey = $h.key; name = "E2E Host"; role = "host"; pairedAt = $now }) | Set-Content "$cliCfg\trusted.json"
@{ role = "host"; settings = @{ deviceName = "E2E Host"; port = $Port; discoveryEnabled = $false; autoStart = $true } } | ConvertTo-Json | Set-Content "$hostCfg\settings.json"
$primary = @{ x = 0; y = 0; width = [In]::GetSystemMetrics(0); height = [In]::GetSystemMetrics(1) }
ConvertTo-Json -Depth 4 @(@{ id = $c.id; name = "E2E Client"; x = -1.0 * $primary.width; y = 0.0;
  monitors = @(@{ x = 0; y = 0; width = $primary.width; height = $primary.height; dpi = 96; primary = $true; name = "E2E" }) }) |
  Set-Content "$hostCfg\layout.json"

$env:APPDATA = "$work\roam"; $env:LOCALAPPDATA = "$work\local"; $env:MOUSH_TEST_ACCEPT_INJECTED = "1"
$gui = Start-Process "$Bin\Moush.exe" -PassThru -ArgumentList "--minimized"
Remove-Item Env:\MOUSH_TEST_ACCEPT_INJECTED
Start-Sleep -Seconds 3
$cli = Start-Process "$Bin\moush.exe" -PassThru -WindowStyle Hidden `
  -ArgumentList "client", "127.0.0.1", "--port", "$Port", "--dry-run", "--verbose", "--config-dir", "`"$cliCfg`"" `
  -RedirectStandardError "$work\client.log" -RedirectStandardOutput "$work\client.out"
Start-Sleep -Seconds 2

$cx = [int]($primary.width / 2); $cy = [int]($primary.height / 2)
$orig = Pos; $failures = @()
try {
  [In]::MoveAbs(10, 500); Start-Sleep -Milliseconds 150
  [In]::MoveAbs(0, 500); Start-Sleep -Milliseconds 400
  $parked = Pos
  if ([Math]::Abs($parked.X - $cx) -gt 2 -or [Math]::Abs($parked.Y - $cy) -gt 2) {
    $failures += "crossing: host cursor at ($($parked.X),$($parked.Y)), expected parked at ($cx,$cy)"
  } else {
    [In]::MoveAbs($cx - 20, $cy + 5); Start-Sleep -Milliseconds 150
    [In]::keybd_event(0x87, 0x76, 0, [IntPtr]::Zero); [In]::keybd_event(0x87, 0x76, 2, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    [In]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero); [In]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
    [In]::mouse_event(0x0800, 0, 0, -120, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 150
    [In]::MoveAbs($cx + 100, $cy); Start-Sleep -Milliseconds 400
    $back = Pos
    if ($back.X -gt 5) { $failures += "return: host cursor at ($($back.X),$($back.Y)), expected at the left edge" }
  }
} finally {
  [In]::SetCursorPos($orig.X, $orig.Y) | Out-Null
  Start-Sleep -Milliseconds 300
  Stop-Process -Id $cli.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 300
  Stop-Process -Id $gui.Id -Force -ErrorAction SilentlyContinue
}

$log = Get-Content "$work\client.log" -Raw
$expect = [ordered]@{
  "client received the entry point"   = "move $($primary.width - 1),500"
  "relative move"                     = "move $($primary.width - 21),505"
  "F24 key down"                      = "key vk=0x87 scan=0x76 down"
  "F24 key up"                        = "key vk=0x87 scan=0x76 up"
  "left button down"                  = "Left down"
  "left button up"                    = "Left up"
  "wheel"                             = "wheel Vertical -120"
  "cursor left the client"            = "shared cursor left this computer"
}
foreach ($k in $expect.Keys) {
  $ok = $log.Contains($expect[$k])
  if (-not $ok) { $failures += "missing at client: $k" }
  "{0,-34} {1}" -f $k, $(if ($ok) { "ok" } else { "MISSING" })
}
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
if ($failures.Count) { $failures | ForEach-Object { "FAIL: $_" }; exit 1 }
"PASS"
