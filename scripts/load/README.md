# Kokkeak Load Tests (T-26)

k6 scripts that exercise the main user flows against a running
Kokkeak deployment. Run nightly via CI; failures block the next
release.

## Prerequisites

```bash
# Install k6
scoop install k6           # Windows
brew install k6            # macOS
sudo snap install k6       # Linux

# Generate the seed file from your local JSON-DB
jq '[.users[] | select(.role == "customer") | {username, password}]' \
  data/json_db/user.json > scripts/load/seed.json
```

## Scripts

| Script | Scenario | Steady RPS | Burst RPS |
|---|---|---|---|
| `login.js` | Login + me fetch (cache-bound) | 500 | 1000 |
| `create_order.js` | Register → login → order create (DB-bound) | 100 | n/a |

## Running

```bash
# Local dev stack
k6 run scripts/load/login.js

# Staging
k6 run -e BASE_URL=https://staging.kokkak.example scripts/load/login.js

# Capture raw results for later analysis
k6 run --out json=results/login.json scripts/load/login.js
```

## Targets

Drawn from `KOKKAK_TASKS_PLAN.md` T-26:

| Endpoint class | p99 | Error budget |
|---|---|---|
| Read-heavy (`/users/me`, `/catalog/*`) | < 300ms | < 0.1% |
| Login (`/auth/login`) | < 500ms | < 0.1% |
| Write-heavy (`/orders`, `/payments`) | < 1000ms | < 1% |

## CI integration

The nightly job is defined in `.github/workflows/load-test.yml`
(planned). It runs against a pre-prod deployment spun up from
the same commit. Failures open an issue tagged `regression`.

## Adding new scenarios

1. Pick a single user flow (login, order create, chat send).
2. Reuse `http` + `check` + `sleep` from the existing scripts.
3. Set thresholds that match the endpoint's SLO budget.
4. Document the script here with steady/burst RPS targets.


HOW TO DEPLOY
  WINDOWS
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "scripts/publish-prod.ps1" -OutputZip

    .\scripts\publish-prod.ps1
