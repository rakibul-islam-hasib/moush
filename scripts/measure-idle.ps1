# Idle resource use of the headless engines: a CLI host (real hooks, discovery, layout) and a
# dry-run CLI client connected to it over loopback, both idle. Reports engine start-up time,
# connect time, CPU time, CPU %, context switches per second and memory after a warm-up.
#
#   scripts\measure-idle.ps1                       # 60 s, target\release\moush-cli.exe
#   scripts\measure-idle.ps1 -Seconds 120 -Exe path\to\moush-cli.exe
#
# The two engines get their own throw-away config directories, pre-paired with each other,
# and the client is placed far away from the host's monitors in the layout, so moving the
# mouse during the measurement can never switch to it. Do not touch mouse or keyboard
# while it runs (host hooks see every event and that is not "idle").
param(
    [int]$Seconds = 60,
    [string]$Exe = "",
    [int]$Port = 24931
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $root "target\release\moush-cli.exe" }
if (-not (Test-Path $Exe)) { throw "$Exe not found: cargo build --release -p moush-cli" }
$work = Join-Path ([IO.Path]::GetTempPath()) ("moush-idle-" + [Guid]::NewGuid().ToString("N").Substring(0, 8))
$hostDir = Join-Path $work "host"; $clientDir = Join-Path $work "client"
New-Item -ItemType Directory -Force $hostDir, $clientDir | Out-Null

function Identity([string]$dir) {
    $out = & $Exe identity --config-dir $dir | Out-String
    $id = [regex]::Match($out, "id\s+([0-9a-f]{16})").Groups[1].Value
    $key = [regex]::Match($out, "public key\s+([0-9a-f]{64})").Groups[1].Value
    if (-not $id -or -not $key) { throw "cannot read identity from: $out" }
    return @{ id = $id; key = $key }
}

function Trust([string]$dir, $peer, [string]$role) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $entry = [ordered]@{ id = $peer.id; publicKey = $peer.key; name = "idle-$role"; role = $role; pairedAt = $now; lastSeen = $now; lastAddress = $null }
    ConvertTo-Json -InputObject @($entry) | Set-Content -Encoding utf8 (Join-Path $dir "trusted.json")
}

$h = Identity $hostDir
$c = Identity $clientDir
Trust $hostDir $c "client"
Trust $clientDir $h "host"
# Park the client far outside the host's desktop: never adjacent, never reachable.
$layout = [ordered]@{ id = $c.id; name = "idle-client"; x = -100000.0; y = -100000.0; monitors = @([ordered]@{ x = 0; y = 0; width = 1920; height = 1080; dpi = 96; primary = $true; name = "D" }) }
ConvertTo-Json -Depth 4 -InputObject @($layout) | Set-Content -Encoding utf8 (Join-Path $hostDir "layout.json")
# A truly empty stdin: the CLI runs until killed when stdin is at EOF (an empty line = Enter = stop).
$empty = Join-Path $work "stdin.txt"; [IO.File]::WriteAllBytes($empty, [byte[]]@())

function WaitFor([string]$file, [string]$pattern, [double]$timeoutS) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $timeoutS) {
        if ((Test-Path $file) -and (Select-String -Path $file -Pattern $pattern -Quiet)) { return $sw.Elapsed.TotalMilliseconds }
        Start-Sleep -Milliseconds 5
    }
    throw "timed out waiting for '$pattern' in $file"
}

$hostOut = Join-Path $work "host.out"; $clientOut = Join-Path $work "client.out"
$hp = $null; $cp = $null
try {
    $hp = Start-Process $Exe -ArgumentList "host", "--port", $Port, "--config-dir", "`"$hostDir`"" -RedirectStandardInput $empty -RedirectStandardOutput $hostOut -RedirectStandardError (Join-Path $work "host.err") -NoNewWindow -PassThru
    $hostStart = WaitFor $hostOut "hosting on port" 10
    $cp = Start-Process $Exe -ArgumentList "client", "127.0.0.1:$Port", "--dry-run", "--config-dir", "`"$clientDir`"" -RedirectStandardInput $empty -RedirectStandardOutput $clientOut -RedirectStandardError (Join-Path $work "client.err") -NoNewWindow -PassThru
    $connect = WaitFor $clientOut "Connected" 10
    Write-Host ("host engine ready {0:N0} ms after launch; client connected {1:N0} ms after launch" -f $hostStart, $connect)

    Start-Sleep -Seconds 5   # warm-up: handshake, first layout, first pings
    $procs = @{ host = Get-Process -Id $hp.Id; client = Get-Process -Id $cp.Id }
    $cores = [Environment]::ProcessorCount
    function Switches([int]$procId) {
        # Sum of context switches of all threads (each one is a wake-up).
        $sum = 0
        try {
            # Instances are "moush/<thread>" and "moush#1/<thread>"; map them to process ids.
            $samples = (Get-Counter "\Thread(moush*)\Context Switches/sec", "\Thread(moush*)\ID Process" -ErrorAction Stop).CounterSamples
            $inst = { param($path) [regex]::Match($path, "\(([^)]*)\)").Groups[1].Value }
            $ids = @{}
            foreach ($s in $samples | Where-Object { $_.Path -like "*id process" }) { $ids[(& $inst $s.Path)] = $s.CookedValue }
            foreach ($s in $samples | Where-Object { $_.Path -like "*context switches/sec" }) {
                if ($ids[(& $inst $s.Path)] -eq $procId) { $sum += $s.CookedValue }
            }
        } catch { return [double]::NaN }
        return $sum
    }
    $t0 = @{}; foreach ($k in $procs.Keys) { $procs[$k].Refresh(); $t0[$k] = $procs[$k].TotalProcessorTime }
    $wall = [Diagnostics.Stopwatch]::StartNew()
    $cs = @{ host = @(); client = @() }
    while ($wall.Elapsed.TotalSeconds -lt $Seconds) {
        foreach ($k in $procs.Keys) { $cs[$k] += (Switches $procs[$k].Id) }
        Start-Sleep -Seconds ([math]::Min(10, [math]::Max(1, $Seconds / 6)))
    }
    $elapsed = $wall.Elapsed.TotalSeconds
    foreach ($k in "host", "client") {
        $p = $procs[$k]; $p.Refresh()
        $cpu = ($p.TotalProcessorTime - $t0[$k]).TotalMilliseconds
        $avgCs = ($cs[$k] | Where-Object { -not [double]::IsNaN($_) } | Measure-Object -Average).Average
        Write-Host ("{0,-6} CPU {1,6:N1} ms in {2:N0} s = {3:N4} % of one core ({4:N5} % of {5} cores); ~{6:N1} context switches/s; working set {7:N1} MB, private {8:N1} MB, {9} threads" -f `
            $k, $cpu, $elapsed, (100 * $cpu / 1000 / $elapsed), (100 * $cpu / 1000 / $elapsed / $cores), $cores, $avgCs, ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB), $p.Threads.Count)
    }
} finally {
    foreach ($p in $cp, $hp) { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
    Start-Sleep -Milliseconds 200
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
