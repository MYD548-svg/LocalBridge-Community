[CmdletBinding()]
param()
$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
& node scripts/test/build-evidence.mjs artifacts
if ($LASTEXITCODE -ne 0) { throw "Artifact checksum generation failed" }
