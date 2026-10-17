<#
.SYNOPSIS
Every check a change must pass before it is pushed, in the order CI would
run them, stopping at the first one that fails.

.DESCRIPTION
1. cargo fmt --check
2. cargo test             (WINCRAFT_INSTANCE removed, scratch LOCALAPPDATA)
3. cargo clippy --all-targets -- -D warnings
4. cargo build --release --locked, with -D warnings, into target\verify
5. cargo llvm-cov, failing under the line coverage in tools\coverage-floor.txt
   (skipped with a note when cargo-llvm-cov is not installed)
6. tools\smoke.ps1, only with -Smoke (it shows tray icons for a moment)

-Quick skips steps 4 and 5. One line is printed per step, with its time.
The exit code is 0 when every step passed and 1 at the first failure.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools\check.ps1 -Quick
#>
[CmdletBinding()]
param(
    [switch]$Quick,
    [switch]$Smoke
)

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$scratch = Join-Path ([IO.Path]::GetTempPath()) 'wincraft-check'
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$covTarget = if ($env:WINCRAFT_COV_TARGET) { $env:WINCRAFT_COV_TARGET } else { 'target/cov' }

$total = [Diagnostics.Stopwatch]::StartNew()
$failed = $false

# Runs one step as a command line. $Env holds the environment changes for it;
# $null removes a variable. The previous values come back afterwards. The
# output goes to a file through cmd, not through PowerShell's pipeline: a
# process a test leaves running would hold the pipeline open until it ends.
function Invoke-Step {
    param([string]$Name, [string]$Command, [hashtable]$Env = @{})
    if ($script:failed) { return }
    $saved = @{}
    foreach ($key in $Env.Keys) {
        $saved[$key] = [Environment]::GetEnvironmentVariable($key)
        [Environment]::SetEnvironmentVariable($key, $Env[$key])
    }
    $log = Join-Path $scratch ("step-" + ($Name -replace '[^A-Za-z0-9]', '_') + '.log')
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $code = 1
    try {
        cmd /c "$Command > `"$log`" 2>&1"
        $code = $LASTEXITCODE
        if ($null -eq $code) { $code = 0 }
    } catch {
        $_ | Out-String | Add-Content -Path $log
    } finally {
        foreach ($key in $saved.Keys) {
            [Environment]::SetEnvironmentVariable($key, $saved[$key])
        }
    }
    $seconds = [math]::Round($timer.Elapsed.TotalSeconds, 1)
    if ($code -eq 0) {
        Write-Host ("ok    {0,-12} {1,6}s" -f $Name, $seconds)
    } else {
        Write-Host ("FAIL  {0,-12} {1,6}s  (exit {2})" -f $Name, $seconds, $code)
        Get-Content -Path $log -Tail 12 | ForEach-Object { Write-Host "      $_" }
        $script:failed = $true
    }
}

function Skip-Step {
    param([string]$Name, [string]$Why)
    Write-Host ("skip  {0,-12}         {1}" -f $Name, $Why)
}

$floorFile = Join-Path $PSScriptRoot 'coverage-floor.txt'
$floor = (Get-Content -Path $floorFile -Raw).Trim()

Invoke-Step 'fmt' 'cargo fmt --check'

# A shell that still has WINCRAFT_INSTANCE set would make some tests see a
# test instance; the scratch LOCALAPPDATA keeps the tests off the real profile.
$testEnv = @{ WINCRAFT_INSTANCE = $null; LOCALAPPDATA = $scratch }
Invoke-Step 'test' 'cargo test' $testEnv

Invoke-Step 'clippy' 'cargo clippy --all-targets -- -D warnings'

if ($Quick) {
    Skip-Step 'release' '-Quick'
    Skip-Step 'coverage' '-Quick'
} else {
    Invoke-Step 'release' 'cargo build --release --locked' @{
        RUSTFLAGS = '-D warnings'
        CARGO_TARGET_DIR = 'target\verify'
    }
    cargo llvm-cov --version *> $null
    if ($LASTEXITCODE -eq 0) {
        Invoke-Step 'coverage' "cargo llvm-cov --summary-only --fail-under-lines $floor" @{
            CARGO_TARGET_DIR = $covTarget
            WINCRAFT_INSTANCE = $null
            LOCALAPPDATA = $scratch
        }
    } else {
        Skip-Step 'coverage' 'cargo-llvm-cov is not installed'
    }
}

if ($Smoke) {
    Invoke-Step 'smoke' "powershell -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $PSScriptRoot 'smoke.ps1')`""
}

Write-Host ("total {0:N1}s" -f $total.Elapsed.TotalSeconds)
if ($failed) { exit 1 }
exit 0
