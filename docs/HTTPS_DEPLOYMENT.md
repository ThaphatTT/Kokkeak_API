# HTTPS Deployment Guide

> KOKKAK_API — Rust + axum-server + rustls
> Covers: cert generation, Let's Encrypt, reverse proxy, cert rotation.

## Overview

The Rust API serves HTTPS via `axum-server 0.7` + `rustls 0.23`. There is
no native plain-HTTP fallback in production — `KOKKAK_ENVIRONMENT=production`
**requires** TLS enabled (enforced at startup, see T-11 in `AGENTS.md` §11.7).

```
                                 ┌──────────────────┐
   client (HTTPS :443) ─────────►│  kokkak-api      │
                                 │  (rustls+axum)   │
   client (HTTP  :80)  ───308───►│  redirect server │ ← only when
                                 │  (background)    │   REDIRECT_FROM_PORT > 0
                                 └──────────────────┘
                                          │
                                  ┌───────┴───────┐
                                  ▼               ▼
                              SQL Server      Mongo / Redis / NATS
                              (tiberius)      (chat, cache, queue)
```

## 1. Generate a self-signed cert (dev / staging only)

```bash
mkdir -p ./dev-certs
openssl req -x509 -newkey rsa:4096 -nodes \
  -keyout ./dev-certs/key.pem \
  -out ./dev-certs/cert.pem \
  -days 90 \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"
```

Add to `.env`:
```env
KOKKAK_ENVIRONMENT=development
KOKKAK_TLS__ENABLED=true
KOKKAK_TLS__CERT_PATH=./dev-certs/cert.pem
KOKKAK_TLS__KEY_PATH=./dev-certs/key.pem
KOKKAK_TLS__REDIRECT_FROM_PORT=8081
KOKKAK_TLS__HSTS_MAX_AGE_SECS=0
KOKKAK_TLS__AUTO_RELOAD=false
KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS=https://app.localhost:3000
```

`redirect_from_port=8081` keeps :80 free for any local webserver you're
using; set to `0` to disable the redirect listener.

## 2. Let's Encrypt (production)

Use `certbot` with the `webroot` or `standalone` plugin. We recommend
`webroot` so certbot doesn't need port 80 open:

```bash
# Install
sudo apt install certbot

# Initial cert (replace example.com with your domain)
sudo certbot certonly --webroot \
  -w /var/www/letsencrypt \
  -d api.example.com

# Certs land in /etc/letsencrypt/live/api.example.com/
#   cert.pem  — leaf + chain
#   fullchain.pem — leaf + intermediate chain
#   privkey.pem  — private key
#
# IMPORTANT: rustls needs the LEAF cert + the chain, so point
# KOKKAK_TLS__CERT_PATH at fullchain.pem (NOT cert.pem).
```

Production `.env`:
```env
KOKKAK_ENVIRONMENT=production
KOKKAK_TLS__ENABLED=true
KOKKAK_TLS__CERT_PATH=/etc/letsencrypt/live/api.example.com/fullchain.pem
KOKKAK_TLS__KEY_PATH=/etc/letsencrypt/live/api.example.com/privkey.pem
KOKKAK_TLS__REDIRECT_FROM_PORT=80
KOKKAK_TLS__HSTS_MAX_AGE_SECS=31536000
KOKKAK_TLS__AUTO_RELOAD=true
KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS=https://app.example.com,https://admin.example.com
```

Auto-renewal via systemd timer (certbot installs this by default):
```bash
sudo systemctl list-timers | grep certbot
# Should show certbot.timer firing twice daily; actual renewal
# happens when <30 days remain.
```

## 3. Cert rotation (T-12)

LE issues 90-day certs. There are two patterns to handle rotation:

### 3a. In-process watcher (default)

Set `KOKKAK_TLS__AUTO_RELOAD=true`. The service:

1. Watches `cert_path` and `key_path` via `notify 6`
2. On any modification, logs a fingerprint diff (old → new)
3. Calls `axum_server::Handle::shutdown()` to drain in-flight requests
4. Exits with success — systemd / k8s restarts the process
5. New process reads the new cert at startup

**Trade-off:** ~1-2s connection blip during the restart loop. Fine for
most APIs; not acceptable for zero-downtime.

### 3b. TLS-terminating sidecar (zero-downtime)

For true zero-downtime rotation, put a reverse proxy in front:

```
                ┌──────────────────────┐
client ──HTTPS──►  nginx / envoy / haproxy  ──HTTP──► kokkak-api
                └──────────────────────┘
                         │
                         └── reloads certs on SIGHUP (zero-downtime)
```

The proxy handles cert reload via its own SIGHUP mechanism; kokkak-api
runs plain HTTP on a private port. Disable TLS in the Rust binary:

```env
KOKKAK_TLS__ENABLED=false
KOKKAK_SERVER__ADDR=127.0.0.1:3001
```

Configure nginx with `proxy_pass http://127.0.0.1:3001;` and
`ssl_certificate` / `ssl_certificate_key` pointing at the LE paths.
Reload nginx on cert change: `nginx -s reload`.

## 4. Reverse proxy (when not using in-process TLS)

### nginx

```nginx
server {
    listen 80;
    server_name api.example.com;
    return 308 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate     /etc/letsencrypt/live/api.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/api.example.com/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    location / {
        proxy_pass         http://127.0.0.1:3001;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto $scheme;
        # SSE / WebSocket
        proxy_buffering    off;
        proxy_read_timeout 1h;
    }
}
```

Reload: `sudo nginx -s reload` (or `systemctl reload nginx`).

### envoy

```yaml
static_resources:
  listeners:
  - name: listener_https
    address: { socket_address: { address: 0.0.0.0, port_value: 443 } }
    filter_chains:
    - transport_socket:
        name: envoy.transport_sockets.tls
        typed_config:
          common_tls_context:
            tls_certificates:
            - certificate_chain: { filename: /etc/letsencrypt/live/api.example.com/fullchain.pem }
              private_key:     { filename: /etc/letsencrypt/live/api.example.com/privkey.pem }
      filters:
      - name: envoy.filters.network.http_connection_manager
        typed_config:
          stat_prefix: ingress_http
          route_config:
            virtual_hosts:
            - name: backend
              domains: ["api.example.com"]
              routes:
              - match: { prefix: "/" }
                route: { cluster: kokkak_api }
          http_filters:
          - name: envoy.filters.http.router
  clusters:
  - name: kokkak_api
    load_assignment:
      cluster_name: kokkak_api
      endpoints:
      - lb_endpoints:
        - endpoint:
            address: { socket_address: { address: 127.0.0.1, port_value: 3001 } }
```

## 5. HSTS tuning

- `KOKKAK_TLS__HSTS_MAX_AGE_SECS=0` → no HSTS header (don't use in prod)
- `KOKKAK_TLS__HSTS_MAX_AGE_SECS=31536000` → 1 year, the OWASP-recommended
  starting value for fresh deployments
- `KOKKAK_TLS__HSTS_MAX_AGE_SECS=63072000` → 2 years, the HSTS preload
  list minimum (only after you're committed to HTTPS forever)

**WARNING:** HSTS is a one-way door. The `includeSubDomains` directive
is intentionally **NOT** added by our code — before enabling it, audit
every subdomain (staging, dev, internal admin) for HTTPS readiness.
A single HTTP-only subdomain will break the moment you enable HSTS
on the parent.

## 6. Verification checklist

After deploying, verify:

```bash
# Cert chain is correct
openssl s_client -connect api.example.com:443 -showcerts < /dev/null

# HSTS header present with expected max-age
curl -sI https://api.example.com/healthz | grep -i strict-transport

# HTTP → HTTPS redirect works
curl -sI http://api.example.com/healthz | head -3
# Expect: HTTP/1.1 308 Permanent Redirect + Location: https://...

# Cert is not close to expiry
echo | openssl s_client -connect api.example.com:443 -servername api.example.com 2>/dev/null \
  | openssl x509 -noout -dates

# Rate-limit header on /healthz (should be there if RATE_LIMIT__ENABLED=true)
curl -sI https://api.example.com/healthz | grep -i ratelimit
```

## 7. Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `failed to read TLS certificate` at startup | Path wrong or file missing | Verify `KOKKAK_TLS__CERT_PATH`; the binary refuses to start with the bad path |
| `no certificates found in /etc/.../cert.pem` | Pointed at leaf-only `cert.pem` instead of `fullchain.pem` | Use `fullchain.pem` from Let's Encrypt |
| Cert rotation not picked up | `KOKKAK_TLS__AUTO_RELOAD=false` | Set to `true`; OR run `systemctl restart kokkak-api` manually |
| `HSTS disabled (max-age = 0)` in logs | `KOKKAK_TLS__HSTS_MAX_AGE_SECS=0` | Set to `31536000` (1 year) in production |
| `port 80 already in use` warning on redirect listener | nginx / another service on :80 | Set `KOKKAK_TLS__REDIRECT_FROM_PORT=0` (the API still serves HTTPS) or stop the conflicting service |
| Browser shows `NET::ERR_CERT_AUTHORITY_INVALID` | Self-signed cert or missing intermediate | For dev: import cert to OS keychain. For prod: ensure `fullchain.pem` is used (not `cert.pem`). |
| `tls_auto_reload_load_from_env_overrides` test fails | `KOKKAK_TLS__AUTO_RELOAD` env var not cleared | `clear_kokkak_env()` in test fixture (see `crates/common/src/config.rs` test suite) |

## 8. References

- `axum-server` 0.7 docs: <https://docs.rs/axum-server/0.7.3>
- `rustls` 0.23 docs: <https://docs.rs/rustls/0.23>
- `tower_governor` 0.4 docs: <https://docs.rs/tower_governor/0.4>
- `notify` 6.x docs: <https://docs.rs/notify/6>
- `certbot` user guide: <https://eff-certbot.readthedocs.io/en/latest/>
- Let's Encrypt rate limits: <https://letsencrypt.org/docs/rate-limits/>
- Mozilla SSL configuration generator: <https://ssl-config.mozilla.org/>
- HSTS preload list: <https://hstspreload.org/>
