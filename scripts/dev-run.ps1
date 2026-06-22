# scripts/dev-run.ps1
# Local dev runner. Loads `.env.dev`, binds port 18080, smoke-tests /healthz.
#
# ponytail: a single self-contained script — no Makefile, no cargo alias, no
# abstraction layer. The whole "dev workflow" is one .ps1 + one .env.
# Upgrade path if we ever want staging + preview envs: extract the env-loading
# block to a shared `scripts/load-env.ps1` and parameterise by filename.
#
# PowerShell 5.1 note: avoid `$()` subexpressions inside double-quoted
# strings (parser bug). Use string concat or pre-compute to a local.

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

# ---- 1. Load .env.dev into process env ----
$envFile = Join-Path $PSScriptRoot '..\.env.dev'
if (-not (Test-Path $envFile)) {
    Write-Error ".env.dev missing at $envFile. Create it from .env.example first."
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

# ---- 2. Verify port is free BEFORE starting ----
$addr = $env:KOKKAK_SERVER__ADDR
$portMatch = [regex]::Match($addr, '(\d+)$')
$port = $portMatch.Groups[1].Value
Write-Host ('[dev-run] KOKKAK_ENVIRONMENT=' + $env:KOKKAK_ENVIRONMENT + ' addr=' + $addr + ' (port ' + $port + ')')

$probe = New-Object System.Net.Sockets.TcpClient
try {
    $probe.Connect('127.0.0.1', [int]$port)
    Write-Error ('[dev-run] Port ' + $port + ' is already in use. Run scripts/dev-stop.ps1 first.')
    exit 1
} catch {
    # Port free — proceed
}

# ---- 3. Start the binary ----
$binPath = Join-Path $PSScriptRoot '..\target\debug\kokkak-api.exe'
if (-not (Test-Path $binPath)) {
    Write-Error ('[dev-run] Binary not found at ' + $binPath + '. Run "cargo build --bin kokkak-api" first.')
    exit 1
}

$logDir = Join-Path $PSScriptRoot '..\logs\dev'
New-Item -ItemType Directory -Path $logDir -Force | Out-Null

$proc = Start-Process -FilePath $binPath -PassThru `
    -RedirectStandardOutput (Join-Path $logDir 'out.log') `
    -RedirectStandardError  (Join-Path $logDir 'err.log')

Write-Host ('[dev-run] started PID=' + $proc.Id + '. Log: ' + $logDir + '\out.log')

# ---- 4. Smoke-test /healthz ----
# T-LocalRun: when KOKKAK_TLS__ENABLED=true, the same port serves
# HTTPS (axum-server::bind_rustls replaces the plain listener). Pick
# the scheme + cert validation policy from the env, and tell the
# user the right URL to open in the browser.
$tlsEnabled = $env:KOKKAK_TLS__ENABLED -eq 'true'
$scheme     = if ($tlsEnabled) { 'https' } else { 'http' }
$url        = $scheme + '://' + $addr + '/healthz'

# .NET's TLS chain validation rejects self-signed certs; -SkipCertificateCheck
# is the dev-run equivalent of curl -k. Safe here: the cert is local-only.
$smokeOpts = @{
    Uri              = $url
    UseBasicParsing  = $true
    TimeoutSec       = 2
}
if ($tlsEnabled) { $smokeOpts['SkipCertificateCheck'] = $true }

$ready = $false
for ($i = 1; $i -le 20; $i++) {
    Start-Sleep -Milliseconds 500
    try {
        $r = Invoke-WebRequest @smokeOpts
        if ($r.StatusCode -eq 200) {
            $elapsed = [math]::Round($i * 0.5, 1)
            Write-Host ('[dev-run] ready after ' + $elapsed + 's — GET ' + $url + ' => 200')
            Write-Host ('[dev-run] OpenAPI: ' + $scheme + '://' + $addr + '/api/openapi.json')
            Write-Host ('[dev-run] Swagger: ' + $scheme + '://' + $addr + '/api/docs/')
            if ($tlsEnabled) {
                Write-Host '[dev-run] TLS: self-signed cert — click through the browser warning.'
            }
            Write-Host ('[dev-run] Tail log: Get-Content "' + $logDir + '\out.log" -Wait')
            $ready = $true
            break
        }
    } catch {
        # server still warming up — try again
    }
}

if (-not $ready) {
    Write-Error ('[dev-run] Server did not become ready in 10s. Check ' + $logDir + '\out.log and ' + $logDir + '\err.log')
    exit 1
}

exit 0
