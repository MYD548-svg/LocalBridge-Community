[CmdletBinding()]
param(
    [string]$LockFile = "provenance/runtime-lock.json"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "=== Phase 3: Runtime Supply Chain Verification ===" -ForegroundColor Cyan
Write-Host "Repo Root: $RepoRoot"

# 1. Check Lock File
$lockPath = Join-Path $RepoRoot $LockFile
if (-not (Test-Path $lockPath)) {
    Write-Error "Runtime lock file not found: $lockPath"
    exit 1
}

$lock = Get-Content $lockPath -Raw | ConvertFrom-Json
Write-Host "Loaded lock file: $LockFile (Community Version: $($lock.community_version))"

$failed = $false

# 2. Test for Forbidden/Backup Binaries
Write-Host "`nChecking for forbidden/backup executables..."
$forbiddenPatterns = @(
    "runtime/tunnel-client/cloudflared.exe",
    "runtime/tunnel-client/cloudflared-manifest.json",
    "runtime/tunnel-client/tunnel-client.upstream-localbridge.exe"
)
foreach ($f in $forbiddenPatterns) {
    $fullPath = Join-Path $RepoRoot $f
    if (Test-Path $fullPath) {
        Write-Error "FAIL: Forbidden binary detected: $f"
        $failed = $true
    }
}

# 3. Verify Registered Components
Write-Host "`nVerifying registered runtime components..."
foreach ($comp in $lock.components) {
    if ($comp.runtime_path) {
        $filePath = Join-Path $RepoRoot $comp.runtime_path
        if (-not (Test-Path $filePath)) {
            Write-Error "FAIL: Missing runtime file: $($comp.runtime_path)"
            $failed = $true
            continue
        }
        
        if ($comp.sha256) {
            $hash = (Get-FileHash -Path $filePath -Algorithm SHA256).Hash.ToLower()
            if ($hash -ne $comp.sha256.ToLower()) {
                Write-Error "FAIL: SHA256 mismatch for $($comp.name) ($($comp.runtime_path))`n  Expected: $($comp.sha256)`n  Actual:   $hash"
                $failed = $true
            } else {
                Write-Host "  [OK] $($comp.name): $($comp.runtime_path) ($hash.Substring(0,12)...)" -ForegroundColor Green
            }
        }
    }
    
    # Sub-artifacts check (e.g. Python DLLs)
    if ($comp.artifacts) {
        $parentDir = Split-Path (Join-Path $RepoRoot $comp.runtime_path) -Parent
        foreach ($prop in $comp.artifacts.PSObject.Properties) {
            $artName = $prop.Name
            $expectedArtHash = $prop.Value
            $artPath = Join-Path $parentDir $artName
            if (Test-Path $artPath) {
                $artHash = (Get-FileHash -Path $artPath -Algorithm SHA256).Hash.ToLower()
                if ($artHash -ne $expectedArtHash.ToLower()) {
                    Write-Error "FAIL: Sub-artifact SHA256 mismatch: $artName`n  Expected: $expectedArtHash`n  Actual:   $artHash"
                    $failed = $true
                } else {
                    Write-Host "  [OK] Sub-artifact $artName verified" -ForegroundColor Green
                }
            }
        }
    }
}

# 4. Unknown Executables Scan in runtime/
Write-Host "`nScanning runtime/ directory for unregistered executables..."
$knownExecutables = @(
    "runtime\tunnel-client\tunnel-client.exe",
    "runtime\python\python.exe",
    "runtime\python\pythonw.exe",
    "runtime\toolbox\bin\aria2c.exe",
    "runtime\toolbox\bin\7z.exe",
    "runtime\toolbox\bin\jq.exe"
)

$allExecutables = Get-ChildItem -Path (Join-Path $RepoRoot "runtime") -Recurse -Filter "*.exe" -File
foreach ($exe in $allExecutables) {
    $relative = (Resolve-Path -Path $exe.FullName -Relative) -replace '^\.\\',''
    if ($relative -notin $knownExecutables) {
        Write-Error "FAIL: Unknown/Unregistered executable detected in runtime: $relative"
        $failed = $true
    }
}

if ($failed) {
    Write-Error "`n=== Runtime Supply Chain Verification FAILED ==="
    exit 1
} else {
    Write-Host "`n=== Runtime Supply Chain Verification PASS ===" -ForegroundColor Green
}
