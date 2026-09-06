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

Write-Host "=== Phase 1: Source-built OpenAI Tunnel Client ===" -ForegroundColor Cyan
Write-Host "Repo Root: $RepoRoot"
Write-Host "Target Commit: $SourceCommit"
Write-Host "Target Version: $ExpectedVersion"

# 1. Check Go compiler
$goCmd = Get-Command "go" -ErrorAction SilentlyContinue
if (-not $goCmd) {
    # Check default Go install locations
    $candidateGo = @(
        "C:\Program Files\Go\bin\go.exe",
        "$env:LOCALAPPDATA\Programs\Go\bin\go.exe",
        "$env:USERPROFILE\go\bin\go.exe"
    )
    foreach ($c in $candidateGo) {
        if (Test-Path $c) {
            $goCmd = $c
            $env:PATH = "$env:PATH;$(Split-Path -Parent $c)"
            break
        }
    }
}

if (-not $goCmd) {
    Write-Error "Go compiler ('go') not found in PATH or standard directories. Please install Go (1.24+ / 1.26.2) to build tunnel-client from source."
    exit 1
}

$goVersionOutput = & go version
Write-Host "Detected Go compiler: $goVersionOutput"

# 2. Setup build scratch area
if (-not $WorkDir) {
    $WorkDir = Join-Path $env:TEMP "tunnel-client-src-$SourceCommit"
}

if (-not (Test-Path $WorkDir)) {
    Write-Host "Fetching OpenAI tunnel-client source archive for commit $SourceCommit..."
    $archiveUrl = "https://codeload.github.com/openai/tunnel-client/zip/$SourceCommit"
    $zipPath = Join-Path $env:TEMP "tunnel-client-$SourceCommit.zip"
    
    Invoke-WebRequest -Uri $archiveUrl -OutFile $zipPath -UseBasicParsing
    Expand-Archive -Path $zipPath -DestinationPath (Split-Path -Parent $WorkDir) -Force
    Remove-Item -Force $zipPath
}

$extractedDirs = Get-ChildItem (Split-Path -Parent $WorkDir) -Directory | Where-Object { $_.Name -like "tunnel-client*" }
$tunnelSrcDir = $extractedDirs[0].FullName
Write-Host "Source directory: $tunnelSrcDir"

# 3. Verify Version and go.mod
$versionFile = Join-Path $tunnelSrcDir "pkg\version\VERSION"
if (Test-Path $versionFile) {
    $actualVersion = (Get-Content $versionFile -Raw).Trim()
    if ($actualVersion -ne $ExpectedVersion) {
        Write-Error "Version mismatch! Expected: $ExpectedVersion, Found: $actualVersion"
        exit 1
    }
    Write-Host "Verified VERSION: $actualVersion" -ForegroundColor Green
} else {
    Write-Warning "pkg\version\VERSION not found at $versionFile"
}

# 4. Optional Native tests
if (-not $SkipTests) {
    Write-Host "Running native tunnel-client tests..."
    Push-Location $tunnelSrcDir
    try {
        & go test ./...
        if ($LASTEXITCODE -ne 0) {
            Write-Error "OpenAI tunnel-client tests failed! BUILD_GATE = FAIL"
            exit 1
        }
        Write-Host "All tunnel-client tests PASS!" -ForegroundColor Green
    } finally {
        Pop-Location
    }
}

# 5. Build tunnel-client.exe
$artifactsDir = Join-Path $RepoRoot "artifacts"
New-Item -ItemType Directory -Force $artifactsDir | Out-Null
$targetBinary = Join-Path $artifactsDir "tunnel-client.exe"

Write-Host "Compiling tunnel-client.exe..."
$env:CGO_ENABLED = "0"
$env:GOOS = "windows"
$env:GOARCH = "amd64"

Push-Location $tunnelSrcDir
try {
    & go build `
      -mod=readonly `
      -trimpath `
      -buildvcs=false `
      -ldflags "-s -w -X github.com/openai/tunnel-client/pkg/version.GitSHA=$SourceCommit" `
      -o $targetBinary `
      ./cmd/client

    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $targetBinary)) {
        Write-Error "Failed to compile tunnel-client.exe!"
        exit 1
    }
} finally {
    Pop-Location
}

Write-Host "Compiled binary created: $targetBinary" -ForegroundColor Green

# 6. Verify binary capabilities
$verOutput = & $targetBinary --version
Write-Host "Binary version check: $verOutput"

# 7. Compute SHA256
$hashObj = Get-FileHash -Path $targetBinary -Algorithm SHA256
$newSha256 = $hashObj.Hash.ToLower()
Write-Host "New tunnel-client.exe SHA256: $newSha256" -ForegroundColor Yellow

# 8. Record Provenance
$provenanceDir = Join-Path $RepoRoot "provenance"
New-Item -ItemType Directory -Force $provenanceDir | Out-Null
$provenance = @{
    component = "openai-tunnel-client"
    source = "https://github.com/openai/tunnel-client"
    version = $ExpectedVersion
    commit = $SourceCommit
    compiler = ($goVersionOutput -replace "`r`n","")
    target = "windows-amd64"
    cgo = $false
    sha256 = $newSha256
    built_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssK")
}
$provenance | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $provenanceDir "tunnel-client.json")
Write-Host "Provenance recorded in provenance/tunnel-client.json"

# 9. Verify binary source metadata using go version -m
$goVersionM = & go version -m $targetBinary | Out-String
$goVersionM | Set-Content -Encoding UTF8 (Join-Path $artifactsDir "tunnel-client-go-version.txt")

# 10. Copy to runtime staging
$runtimeDir = Join-Path $RepoRoot "runtime\tunnel-client"
New-Item -ItemType Directory -Force $runtimeDir | Out-Null
Copy-Item -Path $targetBinary -Destination (Join-Path $runtimeDir "tunnel-client.exe") -Force
Write-Host "Updated runtime\tunnel-client\tunnel-client.exe" -ForegroundColor Green

# 11. Optional update to bundle.rs and runtime-manifest.toml
if ($UpdateBundleRs) {
    $bundleRsPath = Join-Path $RepoRoot "src-tauri\src\tunnel\bundle.rs"
    if (Test-Path $bundleRsPath) {
        $content = Get-Content $bundleRsPath -Raw
        $updatedContent = [regex]::Replace($content, 'pub\(crate\) const TUNNEL_CLIENT_SHA256: &str =\s*"[0-9a-fA-F]+";', "pub(crate) const TUNNEL_CLIENT_SHA256: &str = `"$newSha256`";")
        $updatedContent | Set-Content -Encoding UTF8 $bundleRsPath
        Write-Host "Updated TUNNEL_CLIENT_SHA256 in $bundleRsPath" -ForegroundColor Green
    }
    
    $manifestPath = Join-Path $RepoRoot "runtime-manifest.toml"
    if (Test-Path $manifestPath) {
        $mContent = Get-Content $manifestPath -Raw
        $mUpdated = [regex]::Replace($mContent, 'executable_sha256 = "[0-9a-fA-F]+"', "executable_sha256 = `"$newSha256`"")
        $mUpdated = [regex]::Replace($mUpdated, 'vendoring = "binary"', 'vendoring = "source-built"')
        $mUpdated | Set-Content -Encoding UTF8 $manifestPath
        Write-Host "Updated executable_sha256 and vendoring in $manifestPath" -ForegroundColor Green
    }
}

Write-Host "=== Phase 1 PASS ===" -ForegroundColor Green
