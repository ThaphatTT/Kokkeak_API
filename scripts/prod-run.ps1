$ErrorActionPreference = "Stop"

try {
    $projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

    Set-Location $projectRoot

    $envFile = Join-Path $projectRoot ".env.production"
    $exeFile = Join-Path $projectRoot "kokkak-api.exe"

    if (-not (Test-Path $envFile)) {
        throw "[prod-run] .env.production not found at $envFile"
    }

    if (-not (Test-Path $exeFile)) {
        throw "[prod-run] kokkak-api.exe not found at $exeFile"
    }

    $env:KOKKAK_ENV_FILE = $envFile

    Write-Host "[prod-run] project root: $projectRoot"
    Write-Host "[prod-run] using env file: $envFile"
    Write-Host "[prod-run] starting: $exeFile"
    Write-Host ""

    & $exeFile
}
catch {
    Write-Host ""
    Write-Host "ERROR:" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
}
finally {
    Write-Host ""
    Read-Host "Press Enter to close"
}
