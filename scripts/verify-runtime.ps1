[CmdletBinding()]
param([string]$LockFile = "provenance/runtime-lock.json", [switch]$BundledOnly)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
$arguments = @("scripts/test/runtime-integrity.mjs", "--lock", $LockFile)
if ($BundledOnly) { $arguments += "--bundled-only" }
& node @arguments
if ($LASTEXITCODE -ne 0) { throw "Runtime integrity verification failed" }
