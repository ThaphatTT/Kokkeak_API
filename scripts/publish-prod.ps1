<#
.SYNOPSIS
    Publish Kokkeak API: tests + build + package + zip -> Desktop.

.DESCRIPTION
    Pipeline:
      1. cargo test --workspace                  (gate; -SkipTests to bypass)
      2. cargo build --release --bin kokkak-api
      3. Package to <OutputDir>\: binary + .env.production + prod-run.ps1
                                       + VERSION + MANIFEST.txt + logs\
      4. Security gate: refuse to ship if cert/source artifacts leak
      5. Always create <OutputDir>.zip (single transportable artifact)

    Default locations:
      folder: $env:USERPROFILE\Desktop\KokkeakDeploy\
      zip:    $env:USERPROFILE\Desktop\KokkeakDeploy.zip

    Each run OVERWRITES the previous — no timestamped accumulation.
    VERSION file inside the folder still carries commit + build date
    so deploy history is preserved without filesystem clutter.

.PARAMETER OutputDir
    Folder to write the package into. Default: $env:USERPROFILE\Desktop\KokkeakDeploy

.PARAMETER SkipBuild
    Skip `cargo build --release` (use when iterating on packaging only).

.PARAMETER SkipTests
    Skip `cargo test --workspace` (NOT recommended — CI is the proper gate).

.EXAMPLE
    PS> .\scripts\publish-prod.ps1
    Standard publish: tests + build + package + zip on Desktop.

.EXAMPLE
    PS> .\scripts\publish-prod.ps1 -SkipTests -SkipBuild
    Re-package existing release binary + zip without rebuilding.

.EXAMPLE
    PS> .\scripts\publish-prod.ps1 -OutputDir C:\staging\kokkeak
    Publish to a custom directory (zip lands at C:\staging\kokkeak.zip).

.NOTES
    Architecture: IIS terminates TLS in front (cert in Windows Cert Store).
    Rust API serves HTTP-only on 127.0.0.1:18080. The private key NEVER
    leaves Windows Cert Store — the security gate enforces this.
#>
[CmdletBinding()]
param(
    [string]$OutputDir = (Join-Path $env:USERPROFILE "Desktop\KokkeakDeploy"),
    [switch]$SkipBuild,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

# Locate project root (scripts/..).
$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $projectRoot

# Build metadata (commit + version + timestamp embedded in VERSION + MANIFEST).
$version   = "0.1.0"
$timestamp = Get-Date -Format "yyyy-MM-ddTHH-mm-ss"
$gitCommit = (& git rev-parse --short HEAD 2>$null) -replace "`r?`n",""
if ([string]::IsNullOrWhiteSpace($gitCommit)) { $gitCommit = "unknown" }

$zipPath = "$OutputDir.zip"

# ---- helpers ----
function Step([string]$msg) {
    Write-Host ""
    Write-Host "[publish] $msg" -ForegroundColor Cyan
}
function Die([string]$msg) {
    Write-Host ""
    Write-Host "[publish] FAIL: $msg" -ForegroundColor Red
    exit 1
}

# ---- 1. Tests (gate by default) ----
if (-not $SkipTests) {
    Step "cargo test --workspace"
    cargo test --workspace --no-fail-fast
    if ($LASTEXITCODE -ne 0) {
        Die "cargo test FAILED. Use -SkipTests if you accept the risk."
    }
}

# ---- 2. Build ----
if (-not $SkipBuild) {
    Step "cargo build --release --bin kokkak-api"
    cargo build --release --bin kokkak-api
    if ($LASTEXITCODE -ne 0) { Die "cargo build FAILED" }
    if (-not (Test-Path "target\release\kokkak-api.exe")) {
        Die "binary not found at target\release\kokkak-api.exe after build"
    }
    Step "copy binary to scripts\kokkak-api.exe"
    Copy-Item "target\release\kokkak-api.exe" "scripts\kokkak-api.exe" -Force
}

# ---- 3. Package (always overwrites) ----
Step "package to $OutputDir"
# Wipe the previous content of THIS folder only — leaves everything else alone.
if (Test-Path $OutputDir) {
    Get-ChildItem -Path $OutputDir -Force | Remove-Item -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
New-Item -ItemType Directory -Force -Path "$OutputDir\logs" | Out-Null

Copy-Item "scripts\kokkak-api.exe"  $OutputDir
Copy-Item ".env.production"          $OutputDir
Copy-Item "scripts\prod-run.ps1"    $OutputDir

@"
build=$version
commit=$gitCommit
date=$timestamp
"@ | Set-Content (Join-Path $OutputDir "VERSION")

# ---- 4. Security gate ----
# Refuse to ship if cert / private key / source artifacts / dev config
# leaked into the package. Fail loud, not silent — the 2026-06-27 private
# key leak via chat attachment started from a "harmless" copy step.
Step "security gate"
$forbidden = @(
    "*.pem",  "*.key", "*.pfx", "*.p12",   # TLS / private key
    "*.crt",  "*.cer",                      # alternative cert formats
    ".env.dev", ".env.example",             # dev config / template
    "Cargo.toml", "Cargo.lock",              # source markers
    "target", "crates", "docs", ".git",     # source artifacts
    "*.bak", "*.log", "*.tmp"               # backup / log / temp
)
$violations = @()
foreach ($pattern in $forbidden) {
    Get-ChildItem -Path $OutputDir -Filter $pattern -Recurse -Force -ErrorAction SilentlyContinue |
        ForEach-Object { $violations += $_.FullName }
}
if ($violations.Count -gt 0) {
    Write-Host ""
    Write-Host "[publish] SECURITY GATE FAILED:" -ForegroundColor Red
    $violations | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Die "forbidden artifacts present \u2014 refusing to ship"
}

# ---- 5. Verify required entries ----
Step "verify required entries"
$required = @("kokkak-api.exe", ".env.production", "prod-run.ps1", "VERSION", "logs")
foreach ($name in $required) {
    if (-not (Test-Path (Join-Path $OutputDir $name))) {
        Die "missing required entry: $name"
    }
}

# ---- 6. Manifest with per-file SHA256 ----
Step "write MANIFEST.txt"
$manifest = @(
    "package=kokkeak-api"
    "version=$version"
    "commit=$gitCommit"
    "build_date=$timestamp"
    ""
    "files:"
)
Get-ChildItem -Path $OutputDir -Recurse -File | Sort-Object FullName | ForEach-Object {
    $hash = (Get-FileHash -Algorithm SHA256 $_.FullName).Hash
    $rel  = $_.FullName.Substring($OutputDir.Length + 1)
    $manifest += ("{0}  {1}" -f $hash.Substring(0,16), $rel)
}
$manifest | Set-Content (Join-Path $OutputDir "MANIFEST.txt")

# ---- 7. Always create zip (single transportable artifact) ----
Step "create zip"
if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Compress-Archive -Path $OutputDir -DestinationPath $zipPath -Force
$zipHash = (Get-FileHash -Algorithm SHA256 $zipPath).Hash

# ---- Done ----
Write-Host ""
Write-Host "[publish] DONE" -ForegroundColor Green
Write-Host "  folder: $OutputDir"
Write-Host "  zip:    $zipPath"
Write-Host "  sha256: $zipHash"
Write-Host ""
Get-ChildItem $OutputDir | Format-Table Name, Length -AutoSize | Out-Host
Write-Host ""
Write-Host "Next:" -ForegroundColor Yellow
Write-Host "  Copy $zipPath to server"
Write-Host "  Extract to D:\Apps\Kokkeak\"
Write-Host "  curl.exe -k -i https://sdplao.com/healthz"

# Keep window open if launched by double-click (powershell.exe ConsoleHost).
# No-op for IDE/terminal/automated contexts where $Host.UI.RawUI is null.
# Terminal users can Ctrl+C or press Enter to dismiss — small price for
# not losing output to a closed window.
if ($Host.UI.RawUI) {
    Read-Host "`nPress Enter to close"
}
