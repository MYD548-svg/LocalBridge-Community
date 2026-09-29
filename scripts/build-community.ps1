[CmdletBinding()]
param([string]$GoPath = "", [string]$CargoPath = "")
$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
if ($GoPath) { $env:PATH = "$GoPath;$env:PATH" }
if ($CargoPath) { $env:PATH = "$CargoPath;$env:PATH" }
$previousProfile = $env:LOCALBRIDGE_BUILD_PROFILE
try {
    $env:LOCALBRIDGE_BUILD_PROFILE = "community"
    & node scripts/test/ci-gate.mjs
    if ($LASTEXITCODE -ne 0) { throw "Community gate failed; see tests/artifacts/ci/TEST-REPORT.json" }
} finally { $env:LOCALBRIDGE_BUILD_PROFILE = $previousProfile }
