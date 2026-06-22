# scripts/dev-stop.ps1
# Stops the local dev instance started by dev-run.ps1.

$ErrorActionPreference = 'Stop'

$procs = Get-Process -Name 'kokkak-api' -ErrorAction SilentlyContinue
if (-not $procs) {
    Write-Host "[dev-stop] no kokkak-api.exe process running."
    exit 0
}

foreach ($p in $procs) {
    Write-Host "[dev-stop] stopping PID=$($p.Id) (cmdline: $($p.StartInfo.Arguments))"
    Stop-Process -Id $p.Id -Force
}

Write-Host "[dev-stop] done."
