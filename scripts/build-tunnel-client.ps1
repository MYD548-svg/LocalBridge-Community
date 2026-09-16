[CmdletBinding()]
param(
    [string]$SourceCommit = "8d55683eeef80bc5e360d95abf4692454fafc615",
    [string]$ExpectedVersion = "0.0.11",
    [string]$WorkDir = "",
    [switch]$SkipTests,
    [switch]$UpdateBundleRs
)
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot
function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit $LASTEXITCODE" }
}
if ($SourceCommit -notmatch '^[a-f0-9]{40}$') { throw "A full pinned commit is required" }
foreach ($program in @("go", "git", "node")) { Get-Command $program -ErrorAction Stop | Out-Null }
$goVersionOutput = & go version
if ($LASTEXITCODE -ne 0) { throw "go version failed" }
if (-not $WorkDir) {
    $WorkDir = Join-Path $env:TEMP ("localbridge-tunnel-" + [guid]::NewGuid().ToString("N"))
    Invoke-Checked "git" @("init", $WorkDir)
    Invoke-Checked "git" @("-C", $WorkDir, "remote", "add", "origin", "https://github.com/openai/tunnel-client.git")
    Invoke-Checked "git" @("-C", $WorkDir, "fetch", "--depth=1", "origin", $SourceCommit)
    Invoke-Checked "git" @("-C", $WorkDir, "checkout", "--detach", "FETCH_HEAD")
}
$WorkDir = (Resolve-Path -LiteralPath $WorkDir).Path
Invoke-Checked "node" @("scripts/test/source-check.mjs", $WorkDir, $SourceCommit)
$versionPath = Join-Path $WorkDir "pkg/version/VERSION"
if (-not (Test-Path -LiteralPath $versionPath) -or (Get-Content -LiteralPath $versionPath -Raw).Trim() -ne $ExpectedVersion) { throw "Source VERSION mismatch" }
$artifactsDir = Join-Path $RepoRoot "artifacts"
New-Item -ItemType Directory -Force $artifactsDir | Out-Null
$targetBinary = Join-Path $artifactsDir ("tunnel-client-" + [guid]::NewGuid().ToString("N") + ".exe")
Push-Location $WorkDir
$oldCgo = $env:CGO_ENABLED
$oldOs = $env:GOOS
$oldArch = $env:GOARCH
try {
    # go.mod may select a different toolchain than the shell's default Go.
    $goVersionOutput = & go version
    if ($LASTEXITCODE -ne 0 -or $goVersionOutput -notmatch 'go1\.26\.2\b') { throw "Pinned source requires Go 1.26.2" }
    if (-not $SkipTests) { Invoke-Checked "go" @("test", "-mod=readonly", "./...") }
    else { Write-Host "UPSTREAM_TESTS=NOT_RUN_WINDOWS (complete suite runs in Linux CI)" }
    $env:CGO_ENABLED = "0"
    $env:GOOS = "windows"
    $env:GOARCH = "amd64"
    Invoke-Checked "go" @("build", "-mod=readonly", "-trimpath", "-buildvcs=false", "-ldflags", "-s -w -X github.com/openai/tunnel-client/pkg/version.GitSHA=$SourceCommit", "-o", $targetBinary, "./cmd/client")
} finally {
    $env:CGO_ENABLED = $oldCgo
    $env:GOOS = $oldOs
    $env:GOARCH = $oldArch
    Pop-Location
}
if ((Get-Item -LiteralPath $targetBinary).Length -eq 0) { throw "Empty Tunnel binary" }
Invoke-Checked $targetBinary @("--version")
if ($UpdateBundleRs) {
    Invoke-Checked "node" @("scripts/test/update-tunnel.mjs", $targetBinary, $SourceCommit, $goVersionOutput)
} else {
    Write-Host "Binary built but NOT registered or copied into runtime: $targetBinary"
}
Write-Host "TUNNEL_BUILD=PASS"
