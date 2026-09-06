[CmdletBinding()]
param(
    [string]$OutputFile = "SHA256SUMS.txt"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "=== Generating Checksums ===" -ForegroundColor Cyan

$targets = @(
    "runtime-manifest.toml",
    "runtime-policy.toml",
    "provenance/toolchain.json",
    "provenance/tunnel-client.json",
    "provenance/broker.json",
    "provenance/runtime-lock.json",
    "runtime/tunnel-client/tunnel-client.exe",
    "runtime/tunnel-client/LICENSE",
    "runtime/python/python.exe",
    "runtime/python/python312.dll",
    "runtime/python/python312.zip"
)

# Also check for artifacts directory files
$artifactsDir = Join-Path $RepoRoot "artifacts"
if (Test-Path $artifactsDir) {
    $artFiles = Get-ChildItem -Path $artifactsDir -File
    foreach ($af in $artFiles) {
        $rel = (Resolve-Path -Path $af.FullName -Relative) -replace '^\.\\',''
        if ($rel -notin $targets) {
            $targets += $rel
        }
    }
}

$checksumLines = @()
foreach ($target in $targets) {
    $fullPath = Join-Path $RepoRoot $target
    if (Test-Path $fullPath) {
        $hash = (Get-FileHash -Path $fullPath -Algorithm SHA256).Hash.ToLower()
        $checksumLines += "$hash  $target"
        Write-Host "  $hash  $target"
    }
}

$checksumLines | Set-Content -Encoding UTF8 (Join-Path $RepoRoot $OutputFile)
Write-Host "`nGenerated $OutputFile with $($checksumLines.Count) entries." -ForegroundColor Green
