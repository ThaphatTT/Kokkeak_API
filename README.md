# Kokkeak API

Handyman / technician marketplace backend (Laos) — **Rust + axum + tiberius**.

> **Project rules live in `AGENTS.md`** which is intentionally NOT
> committed (per project convention). Refer to your local copy for
> coding standards, dependency rules, and the full build plan.

## Build

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Run (T01 — minimal)

```bash
cargo run --bin kokkak-api
# → http://0.0.0.0:3000/healthz returns 200 OK
# → http://0.0.0.0:3000/readyz  returns 200 (with empty checks list)
# → http://0.0.0.0:3000/metrics returns Prometheus text
```

## Image downloads (T-23 + T-23-b)

User profile / KYC / bank-book image blobs live in object storage
(`data/uploads/...` on the local FS, or the configured S3 bucket in
production). The API embeds **two** fields per image on every
`GET /api/v1/admin/users/{guid}/detail` response:

| Field | What it is | When the client uses it |
|---|---|---|
| `*_img_path` | relative storage key (`users/{guid}/profile/{uuid}.webp`) | debug / re-upload |
| `*_img_url`  | absolute URL the client pastes into `<img src=...>` | rendering |

### URLs are HMAC-signed (T-23-b)

`GET /files/{*path}?exp={unix}&sig={base64-hmac-sha256}` is the only
way to fetch an image. The signature covers the **path AND the
expiry** so an attacker can't extend a leaked URL or swap paths.
Frontend code does NOT change — paste the URL into `<img src=...>`
exactly as before.

| Env | Purpose |
|---|---|
| `KOKKAK_SERVER__PUBLIC_BASE_URL` | client-facing base URL (e.g. `http://localhost:18080` in dev, `https://api.sdplao.com` in prod). **NOT** the bind addr — production runs the API on a loopback behind IIS / nginx. |
| `KOKKAK_STORAGE__SIGNED_URL_SECRET` | HMAC-SHA256 secret (>= 32 bytes, see AGENTS.md §21.11). Rotation invalidates every URL the API has ever handed out. |
| `KOKKAK_STORAGE__SIGNED_URL_TTL_SECS` | URL lifetime in seconds, default 600 (10 min), range 60..=3600. Pick small enough that a leaked URL has limited blast radius; pick large enough that scrolling a user's six KYC attachments doesn't re-fetch detail. |
| `KOKKAK_STORAGE__LOCAL_PATH` / `KOKKAK_STORAGE__S3_BUCKET` | selects the storage backend. Both adapters go through the same `/files/*` signed route, so the URL contract is identical regardless of backend. |

### Validation rules

`validate()` rejects:
- `KOKKAK_ENVIRONMENT=production + persistent storage (S3/Local)
  + empty PUBLIC_BASE_URL` (every `*_img_url` would be `null`)
- `KOKKAK_ENVIRONMENT=production + signed URL secret < 32 bytes`
  (lets an attacker forge URLs)
- `signed_url_ttl_secs` outside `60..=3600` (limits blast radius)

In dev / unit tests these knobs default to empty, so running
`cargo run --bin kokkak-api` out-of-the-box still works.

### Frontend integration

```jsx
// Just paste the URL the API hands out. No JS changes needed.
<img src={user.profile_img_url} />
```

If the URL expires (10 min later), the browser sees 403; the
client re-fetches `GET /api/v1/admin/users/{guid}/detail` to get
a fresh URL. In practice this is invisible to the user — the
back button / route change triggers the re-fetch.

### Six image kinds

| field | shape |
|---|---|
| `profile_img_*` | primary profile picture |
| `bank_book_img_*` | bankbook cover |
| `id_card_front_*`, `id_card_back_*` | KYC |
| `proof_of_address_*`, `source_of_funds_statement_*` | KYC |

See `crates/api/src/signed_url.rs::signed_image_url` for the
exact composition (path + exp piped through HMAC-SHA256, base64-
URL-safe encoded).
