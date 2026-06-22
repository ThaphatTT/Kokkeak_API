# scripts/prod-run.ps1
# Production runner. Loads `.env.production`, binds port 8080.
# Mirror of dev-run.ps1 — production validation in config.rs rejects
# any missing required fields at startup.

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$envFile = Join-Path $PSScriptRoot '..\.env.production'
if (-not (Test-Path $envFile)) {
    Write-Error ".env.production missing at $envFile."
    exit 1
}

Get-Content $envFile | ForEach-Object {
    $raw = $_
    # Strip inline comment: anything from unquoted '#' to end of line.
    # Mirrors the dotenv convention (https://dotenvx.com/docs/env-file).
    $line = $raw.Trim()
    if (-not $line -or $line.StartsWith('#')) { return }
    $hashIdx = -1
    $inQuote = $false
    for ($k = 0; $k -lt $line.Length; $k++) {
        $ch = $line[$k]
        if ($ch -eq '"') { $inQuote = -not $inQuote }
        elseif ($ch -eq '#' -and -not $inQuote) { $hashIdx = $k; break }
    }
    if ($hashIdx -ge 0) { $line = $line.Substring(0, $hashIdx).TrimEnd() }
    if (-not $line) { return }
    $eq = $line.IndexOf('=')
    if ($eq -lt 1) { return }
    $key = $line.Substring(0, $eq).Trim()
    $val = $line.Substring($eq + 1).Trim().Trim('"')
    [System.Environment]::SetEnvironmentVariable($key, $val, 'Process')
}

$addr = $env:KOKKAK_SERVER__ADDR
Write-Host ('[prod-run] KOKKAK_ENVIRONMENT=' + $env:KOKKAK_ENVIRONMENT + ' addr=' + $addr)

$binPath = Join-Path $PSScriptRoot '..\target\release\kokkak-api.exe'
if (-not (Test-Path $binPath)) {
    Write-Warning ('[prod-run] release binary not found at ' + $binPath + ' — falling back to debug build.')
    $binPath = Join-Path $PSScriptRoot '..\target\debug\kokkak-api.exe'
}
if (-not (Test-Path $binPath)) {
    Write-Error ('[prod-run] binary not found. Build with "cargo build --release --bin kokkak-api".')
    exit 1
}

$logDir = Join-Path $PSScriptRoot '..\logs\prod'
New-Item -ItemType Directory -Path $logDir -Force | Out-Null

$proc = Start-Process -FilePath $binPath -PassThru `
    -RedirectStandardOutput (Join-Path $logDir 'out.log') `
    -RedirectStandardError  (Join-Path $logDir 'err.log')

Write-Host ('[prod-run] started PID=' + $proc.Id + '. Log: ' + $logDir + '\out.log')
