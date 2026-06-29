$ErrorActionPreference = "Stop"

$base = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $base

# Point KOKKAK_ENV_FILE at the project root .env.production (one
# level above scripts/). The Rust binary (main.rs) reads this env
# var before falling back to KOKKAK_ENVIRONMENT-based naming, so
# we don't depend on the working directory to find the config file.
$projectRoot = Split-Path -Parent $base
$envFile = Join-Path $projectRoot '.env.production'
if (-not (Test-Path $envFile)) {
    Write-Error "[prod-run] .env.production not found at $envFile"
    exit 1
}
$env:KOKKAK_ENV_FILE = $envFile
Write-Host "[prod-run] using env file: $envFile"

.\kokkak-api.exe
