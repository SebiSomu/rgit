<#
.SYNOPSIS
    Test harness for rgit — runs commands in an isolated sandbox.

.DESCRIPTION
    Builds the project (if needed) and runs the rgit binary inside
    a 'test-sandbox/' directory so the project's real .git/ is never touched.

.EXAMPLE
    .\test.ps1 init
    .\test.ps1 add .
    .\test.ps1 commit -m "first commit"
    .\test.ps1 branch feature
    .\test.ps1 --clean            # wipe sandbox and start fresh
#>

param(
    [switch]$Clean,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$RgitArgs
)

$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot
$Sandbox     = Join-Path $ProjectRoot "test-sandbox"
$Binary      = Join-Path $ProjectRoot "target\debug\rgit-main.exe"

# ── Handle --clean ────────────────────────────────────────────────────
if ($Clean) {
    if (Test-Path $Sandbox) {
        Remove-Item -Recurse -Force $Sandbox
        Write-Host "[test] Sandbox cleaned." -ForegroundColor Yellow
    } else {
        Write-Host "[test] Sandbox does not exist, nothing to clean." -ForegroundColor DarkYellow
    }
    # If no other args, stop here
    if (-not $RgitArgs -or $RgitArgs.Count -eq 0) { exit 0 }
}

# ── Build ─────────────────────────────────────────────────────────────
Write-Host "[test] Building rgit..." -ForegroundColor Cyan
Push-Location $ProjectRoot
try {
    $prevPref = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    cargo build 2>&1 | ForEach-Object { Write-Host "  $_" }
    $ErrorActionPreference = $prevPref
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[test] Build failed!" -ForegroundColor Red
        exit 1
    }
} finally {
    Pop-Location
}

# ── Ensure sandbox exists ─────────────────────────────────────────────
if (-not (Test-Path $Sandbox)) {
    New-Item -ItemType Directory -Path $Sandbox | Out-Null
    Write-Host "[test] Created sandbox: $Sandbox" -ForegroundColor Green
}

# ── Run rgit in sandbox ──────────────────────────────────────────────
Write-Host "[test] Running: rgit $($RgitArgs -join ' ')" -ForegroundColor Cyan
Write-Host "       (cwd: $Sandbox)" -ForegroundColor DarkGray
Write-Host ""

Push-Location $Sandbox
try {
    & $Binary @RgitArgs
    $exitCode = $LASTEXITCODE
} finally {
    Pop-Location
}

exit $exitCode
