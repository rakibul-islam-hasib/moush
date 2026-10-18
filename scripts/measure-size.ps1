# Builds the release app and CLI with an optional profile override and records their sizes
# plus the loopback latency of that build. Used to compare size/speed trade-offs.
#
#   scripts\measure-size.ps1 -Label baseline
#   scripts\measure-size.ps1 -Label opt-s -Config 'profile.release.opt-level="s"'
#   scripts\measure-size.ps1 -Label fat -Config 'profile.release.lto="fat"','profile.release.strip=true'
#
# The app (Moush.exe) and the CLI (moush-cli.exe) are built one after the other and copied
# out of the target directory.
# Needs `dist/` (run `npx vite build` first). Results go to target\size\<Label>\ and one line
# is appended to target\size\results.csv.
param(
    [Parameter(Mandatory = $true)][string]$Label,
    [string[]]$Config = @(),
    [string]$TargetDir = "target\size",
    [switch]$SkipLatency
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
if (-not (Test-Path "dist\index.html")) { throw "dist\ is missing: run 'npx vite build' first" }

$cfgArgs = @()
foreach ($c in $Config) { $cfgArgs += @("--config", $c) }
$out = Join-Path $TargetDir $Label
New-Item -ItemType Directory -Force $out | Out-Null
$env:CARGO_TARGET_DIR = (Resolve-Path -LiteralPath (New-Item -ItemType Directory -Force $TargetDir)).Path

function Build([string[]]$cargoArgs) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & cargo build --release @cfgArgs @cargoArgs 2>&1 | Select-String -Pattern "^(error|warning: unused)" | ForEach-Object { Write-Host $_ }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Label" }
    return [math]::Round($sw.Elapsed.TotalSeconds)
}

# Clean each package's own artifacts first so both are re-linked with this profile
# (cheap: dependencies stay built).
& cargo clean --release -p moush 2>&1 | Out-Null
$tApp = Build @("-p", "moush", "--features", "tauri/custom-protocol")
Copy-Item "$env:CARGO_TARGET_DIR\release\Moush.exe" "$out\Moush.exe" -Force
& cargo clean --release -p moush-cli 2>&1 | Out-Null
$tCli = Build @("-p", "moush-cli")
Copy-Item "$env:CARGO_TARGET_DIR\release\moush-cli.exe" "$out\moush-cli.exe" -Force
if ((Get-FileHash "$out\Moush.exe").Hash -eq (Get-FileHash "$out\moush-cli.exe").Hash) {
    # Never run the GUI by mistake (it would start with the real user configuration).
    throw "app and CLI copies are identical; the CLI was not re-linked"
}

$app = (Get-Item "$out\Moush.exe").Length
$cli = (Get-Item "$out\moush-cli.exe").Length
Write-Host ("{0}: Moush.exe {1:N0} bytes ({2:N2} MB), moush-cli.exe {3:N0} bytes; build {4}s + {5}s" -f $Label, $app, ($app / 1MB), $cli, $tApp, $tCli)

$p50 = ""; $p99 = ""; $bulkP50 = ""; $mbs = ""; $enc = ""; $dec = ""
if (-not $SkipLatency) {
    # Full network path (encrypt, TCP, decrypt, decode, mock inject) at 1 kHz.
    $bench = & "$out\moush-cli.exe" bench --events 3000 2>&1 | Out-String
    $bench | Set-Content "$out\bench.txt"
    if ($bench -match "p50 (\d+) us\s+p95 \d+ us\s+p99 (\d+) us") { $p50 = $Matches[1]; $p99 = $Matches[2] }
    # Input latency during an 8 MB bulk stream, and bulk throughput (crypto-heavy).
    $lb = & cargo test --release @cfgArgs -p moush-core --test loopback feature_traffic -- --nocapture 2>&1 | Out-String
    $lb | Set-Content "$out\loopback.txt"
    if ($lb -match "bulk stream: p50 (\d+) us.*throughput (\d+) MB/s") { $bulkP50 = $Matches[1]; $mbs = $Matches[2] }
    # CPU-bound transport cost (no network, no wake-ups): what the optimisation level changes.
    $tp = & cargo test --release @cfgArgs -p moush-core --test throughput -- --ignored --nocapture 2>&1 | Out-String
    $tp | Set-Content "$out\throughput.txt"
    if ($tp -match "encode\+encrypt (\d+) MB/s, decrypt\+decode (\d+) MB/s") { $enc = $Matches[1]; $dec = $Matches[2] }
    Write-Host "  bench p50 $p50 us, p99 $p99 us; during bulk p50 $bulkP50 us; bulk $mbs MB/s; crypto $enc / $dec MB/s"
}

$csv = Join-Path $TargetDir "results.csv"
if (-not (Test-Path $csv)) { "label,config,app_bytes,cli_bytes,bench_p50_us,bench_p99_us,bulk_input_p50_us,bulk_mb_s,encrypt_mb_s,decrypt_mb_s" | Set-Content $csv }
('{0},"{1}",{2},{3},{4},{5},{6},{7},{8},{9}' -f $Label, ($Config -join "; "), $app, $cli, $p50, $p99, $bulkP50, $mbs, $enc, $dec) | Add-Content $csv
