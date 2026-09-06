[CmdletBinding()]
param(
    [switch]$SkipTunnelBuild,
    [switch]$SkipRustBuild,
    [switch]$SkipFrontendBuild,
    [string]$GoPath = "",
    [string]$CargoPath = ""
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "       LocalBridge Community Build v1 Orchestrator      " -ForegroundColor Cyan
Write-Host "========================================================" -ForegroundColor Cyan

# Step 1: Toolchain Validation (Phase 0)
Write-Host "`n[Step 1/10] Verifying Toolchains and Environment..." -ForegroundColor Yellow

if ($GoPath) { $env:PATH = "$GoPath;$env:PATH" }
if ($CargoPath) { $env:PATH = "$CargoPath;$env:PATH" }

$nodeVer = & node --version 2>$null
$npmVer = & npm --version 2>$null
$pythonVer = & python --version 2>$null
$gitVer = & "D:\SillyTavern\Git\cmd\git.exe" --version 2>$null

$goCmd = Get-Command "go" -ErrorAction SilentlyContinue
$goVer = if ($goCmd) { & go version } else { $null }

$cargoCmd = Get-Command "cargo" -ErrorAction SilentlyContinue
$cargoVer = if ($cargoCmd) { & cargo --version } else { $null }

Write-Host "  Node.js: $nodeVer"
Write-Host "  npm:     $npmVer"
Write-Host "  Python:  $pythonVer"
Write-Host "  Git:     $gitVer"
Write-Host "  Go:      $(if ($goVer) { $goVer } else { 'Not found in PATH (Deferred/Optional)' })"
Write-Host "  Cargo:   $(if ($cargoVer) { $cargoVer } else { 'Not found in PATH (Deferred/Optional)' })"

# Step 2: Phase 1 - Source-built OpenAI Tunnel Client
Write-Host "`n[Step 2/10] Phase 1: Source-built OpenAI Tunnel Client..." -ForegroundColor Yellow
if (-not $SkipTunnelBuild) {
    if ($goVer) {
        & ".\scripts\build-tunnel-client.ps1" -UpdateBundleRs
        if ($LASTEXITCODE -ne 0) { throw "build-tunnel-client.ps1 failed!" }
    } else {
        Write-Warning "Go compiler not found. Skipping tunnel-client live compilation (retaining registered baseline binary)."
    }
} else {
    Write-Host "  Tunnel Client build skipped by user flag."
}

# Step 3: Phase 3 - Verify Runtime Supply Chain
Write-Host "`n[Step 3/10] Phase 3: Verifying Bundled Runtime Integrity..." -ForegroundColor Yellow
& ".\scripts\verify-runtime.ps1"
if ($LASTEXITCODE -ne 0) { throw "verify-runtime.ps1 failed!" }

# Step 4: Frontend Dependencies (npm ci / install)
Write-Host "`n[Step 4/10] Frontend Dependencies..." -ForegroundColor Yellow
if (-not (Test-Path "node_modules")) {
    Write-Host "  Installing npm dependencies..."
    & npm install
    if ($LASTEXITCODE -ne 0) { throw "npm install failed!" }
} else {
    Write-Host "  node_modules already present."
}

# Step 5: Frontend Tests & Build
Write-Host "`n[Step 5/10] Running Frontend Tests & Type Checking..." -ForegroundColor Yellow
if (-not $SkipFrontendBuild) {
    Write-Host "  Running npm test (vitest)..."
    & npm test
    if ($LASTEXITCODE -ne 0) { throw "npm test failed!" }

    Write-Host "  Running npm run build (tsc + vite)..."
    & npm run build
    if ($LASTEXITCODE -ne 0) { throw "npm run build failed!" }
    Write-Host "  Frontend build completed successfully!" -ForegroundColor Green
} else {
    Write-Host "  Frontend build skipped by user flag."
}

# Step 6: Phase 2 - Rust Core & Privileged Broker (if cargo available)
Write-Host "`n[Step 6/10] Phase 2: Rust Core & Privileged Broker..." -ForegroundColor Yellow
if (-not $SkipRustBuild) {
    if ($cargoVer) {
        Write-Host "  Running cargo test --locked..."
        & cargo test --manifest-path src-tauri/Cargo.toml --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed!" }

        Write-Host "  Building Privileged Broker..."
        & cargo build --manifest-path src-tauri/Cargo.toml --locked --release --bin localbridge-privileged-broker
        if ($LASTEXITCODE -ne 0) { throw "Broker cargo build failed!" }

        Write-Host "  Building LocalBridge desktop app..."
        & cargo build --manifest-path src-tauri/Cargo.toml --locked --release --bin localbridge
        if ($LASTEXITCODE -ne 0) { throw "LocalBridge cargo build failed!" }
    } else {
        Write-Warning "Cargo/Rust not found in PATH. Rust compilation deferred to CI / MSVC environment."
    }
} else {
    Write-Host "  Rust build skipped by user flag."
}

# Step 7: Toolbox preparation (scripts/prepare-toolbox.mjs)
Write-Host "`n[Step 7/10] Preparing Toolbox Resources..." -ForegroundColor Yellow
if (Test-Path "scripts\prepare-toolbox.mjs") {
    try {
        & node scripts/prepare-toolbox.mjs
    } catch {
        Write-Warning "Toolbox download/extraction encountered network condition, continuing."
    }
}

# Step 8: Generate Checksums
Write-Host "`n[Step 8/10] Generating Checksums (SHA256SUMS.txt)..." -ForegroundColor Yellow
& ".\scripts\generate-checksums.ps1"

# Step 9: Validate Sensitive Information Scan
Write-Host "`n[Step 9/10] Running Sensitive Information Scan..." -ForegroundColor Yellow
$thisScriptName = Split-Path -Leaf $MyInvocation.MyCommand.Path
$scanTargets = Get-ChildItem -Path @("provenance", "scripts", "docs") -Recurse -File -Include "*.json","*.ps1","*.md","*.txt" | Where-Object { $_.Name -ne $thisScriptName }
$leakDetected = $false
# Check for concrete sk- tokens or hardcoded bearer tokens
foreach ($f in $scanTargets) {
    $lines = Get-Content $f.FullName
    foreach ($line in $lines) {
        if ($line -match "LOCALBRIDGE_RUNTIME_API_KEY\s*=\s*['""]sk-[a-zA-Z0-9]{20,}" -or $line -match "Bearer\s+eyJ[a-zA-Z0-9_\-\.]{30,}") {
            Write-Error "CRITICAL SECURITY REGRESSION: Secret leak found in $($f.FullName)"
            $leakDetected = $true
        }
    }
}
if ($leakDetected) { throw "Sensitive credentials detected in repository artifacts!" }
Write-Host "  No secret leaks detected. Security scan PASS!" -ForegroundColor Green

# Step 10: Summary
Write-Host "`n========================================================" -ForegroundColor Green
Write-Host "       LocalBridge Community Build v1 Completed         " -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Green
