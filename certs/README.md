# Local TLS certificates (T-LocalRun)

`scripts/prod-run.ps1` needs a TLS cert + key when
`KOKKAK_TLS__ENABLED=true`. Production deploys use real certs from
Let's Encrypt / internal CA — but for **local prod-mode testing** we
self-sign a `localhost` cert per-host.

## One-time setup

```bash
cd Kokkeak_API
mkdir -p certs
openssl req -x509 -newkey rsa:2048 \
  -keyout certs/localhost-key.pem \
  -out certs/localhost.pem \
  -days 365 -nodes \
  -subj '//CN=localhost' \
  -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1'
```

> Git Bash / MSYS path-mangles the `/CN=` prefix. Use `//CN=localhost`
> (double-slash) or run in PowerShell / WSL.

## What the script picks up

`.env.production` points at:
- `KOKKAK_TLS__CERT_PATH=certs/localhost.pem`
- `KOKKAK_TLS__KEY_PATH=certs/localhost-key.pem`

Change those lines if your dev certs live elsewhere.

## Never commit these

Both files are gitignored (`/certs/*` with `certs/README.md` and
`certs/.gitkeep` whitelisted). If you ever generate a real cert by
accident, **rotate immediately** and add a pre-commit hook to block
future leaks.
