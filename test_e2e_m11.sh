#!/usr/bin/env bash
# test_e2e_m11.sh — End-to-end smoke test for M11 (i18n expansion).
#
# Verifies the per-request locale pipeline:
#   1. Server starts with the in-memory translation repo
#      (no SQL Server required).
#   2. Login with no body returns a localized error message.
#   3. The message differs by `Accept-Language` (en / th / lo).
#   4. `?lang=` query parameter overrides `Accept-Language`.
#   5. Unknown locale falls back to English.
#   6. A pre-populated override in the JSON file wins over
#      the file catalog.
#
# Usage:
#     bash test_e2e_m11.sh
#     PORT=8088 bash test_e2e_m11.sh
#
# Exit code: 0 on success, 1 on any failure.

set -euo pipefail

PORT="${PORT:-8088}"
ADDR="127.0.0.1:${PORT}"
BASE="http://${ADDR}"
DATA_DIR="$(mktemp -d -t kokkak-m11.XXXXXX)"
LOG="$(mktemp -t kokkak-m11.log.XXXXXX)"
OVERRIDE_FILE="${DATA_DIR}/translations.json"
SERVER_PID=""

cleanup() {
    if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
        kill "${SERVER_PID}" 2>/dev/null || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    rm -rf "${DATA_DIR}" 2>/dev/null || true
}
trap cleanup EXIT

# ---------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------

# Wait for /healthz to return 200.
wait_for_server() {
    local tries=40
    for ((i = 0; i < tries; i++)); do
        if curl -fsS "${BASE}/healthz" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.25
    done
    echo "FAIL: server did not start within ~10s" >&2
    tail -n 80 "${LOG}" >&2 || true
    return 1
}

# Login with empty body — returns a localized error.
# usage: login_err <accept-language> [?lang=...]
login_err() {
    local accept="$1"
    local query="${2:-}"
    local body
    body=$(curl -sS -X POST \
        -H "content-type: application/json" \
        -H "accept-language: ${accept}" \
        "${BASE}/api/v1/auth/login${query}" \
        -d '{}' || true)
    echo "${body}"
}

assert_equal() {
    local label="$1"
    local actual="$2"
    local expected="$3"
    if [[ "${actual}" != "${expected}" ]]; then
        echo "FAIL: ${label}: expected ${expected}, got ${actual}" >&2
        return 1
    fi
    echo "  ok: ${label}"
}

assert_contains() {
    local label="$1"
    local haystack="$2"
    local needle="$3"
    if [[ "${haystack}" != *"${needle}"* ]]; then
        echo "FAIL: ${label}: expected to contain ${needle}, got ${haystack}" >&2
        return 1
    fi
    echo "  ok: ${label}"
}

assert_not_contains() {
    local label="$1"
    local haystack="$2"
    local needle="$3"
    if [[ "${haystack}" == *"${needle}"* ]]; then
        echo "FAIL: ${label}: expected NOT to contain ${needle}, got ${haystack}" >&2
        return 1
    fi
    echo "  ok: ${label}"
}

extract_msg() {
    # Use a tiny Python expression to be safe with non-ASCII.
    python3 -c '
import json, sys
data = json.loads(sys.stdin.read())
err = data.get("error") or {}
print(err.get("message") or "")
'
}

# ---------------------------------------------------------------
# Pre-populate the per-tenant override for the next test phase.
# ---------------------------------------------------------------
mkdir -p "${DATA_DIR}"
cat > "${OVERRIDE_FILE}" <<'JSON'
{
  "en": {
    "err_auth.invalid_credentials": "[OVERRIDE] credentials rejected"
  }
}
JSON

# ---------------------------------------------------------------
# Start the server
# ---------------------------------------------------------------
echo "==> starting kokkak-api on ${ADDR} (data dir: ${DATA_DIR})"
KOKKAK_SERVER__ADDR="${ADDR}" \
KOKKAK_DATA_DIR__PATH="${DATA_DIR}" \
KOKKAK_AUTH__JWT_SECRET="i18n-e2e-secret" \
KOKKAK_AUTH__ISSUER="kokkak-i18n-e2e" \
KOKKAK_AUTH__ACCESS_TTL_SECS=60 \
KOKKAK_AUTH__REFRESH_TTL_SECS=600 \
RUST_LOG=info \
cargo run --release --bin kokkak-api >"${LOG}" 2>&1 &
SERVER_PID=$!

wait_for_server

# ---------------------------------------------------------------
# Phase 1: Accept-Language routing
# ---------------------------------------------------------------
echo "==> phase 1: Accept-Language routing"

EN_MSG=$(login_err "en" "" | extract_msg)
echo "  en -> ${EN_MSG}"
assert_contains "en uses override" "${EN_MSG}" "[OVERRIDE]"

TH_MSG=$(login_err "th,en;q=0.5" "" | extract_msg)
echo "  th -> ${TH_MSG}"
# The file catalog (Thai) must be picked for the non-overridden
# key. The auth error mapper chose invalid_credentials, which
# is overridden; we check the validation message instead by
# sending a JSON body that triggers validation.
TH_VALIDATION=$(curl -sS -X POST \
    -H "content-type: application/json" \
    -H "accept-language: th,en;q=0.5" \
    "${BASE}/api/v1/auth/register" \
    -d '{"email":"","password":"","display_name":""}' | extract_msg)
echo "  th (validation) -> ${TH_VALIDATION}"
# The Thai file catalog says: "ข้อมูลไม่ผ่านการตรวจสอบ:" for validation.
assert_contains "th uses Thai catalog" "${TH_VALIDATION}" "ข้อมูลไม่ผ่านการตรวจสอบ"

LO_MSG=$(curl -sS -X POST \
    -H "content-type: application/json" \
    -H "accept-language: lo,en;q=0.5" \
    "${BASE}/api/v1/auth/register" \
    -d '{"email":"","password":"","display_name":""}' | extract_msg)
echo "  lo -> ${LO_MSG}"
# Lao file catalog: "ຂໍ້ມູນບໍ່ຜ່ານການກວດສອບ"
assert_contains "lo uses Lao catalog" "${LO_MSG}" "ຂໍ້ມູນບໍ່ຜ່ານການກວດສອບ"

# ---------------------------------------------------------------
# Phase 2: ?lang= overrides Accept-Language
# ---------------------------------------------------------------
echo "==> phase 2: ?lang= overrides Accept-Language"
QUERY_OVERRIDE=$(curl -sS -X POST \
    -H "content-type: application/json" \
    -H "accept-language: th,en;q=0.5" \
    "${BASE}/api/v1/auth/register?lang=lo" \
    -d '{"email":"","password":"","display_name":""}' | extract_msg)
echo "  ?lang=lo with th header -> ${QUERY_OVERRIDE}"
assert_contains "?lang=lo wins over th header" "${QUERY_OVERRIDE}" "ຂໍ້ມູນບໍ່ຜ່ານການກວດສອບ"

# ---------------------------------------------------------------
# Phase 3: Unknown locale falls back to English
# ---------------------------------------------------------------
echo "==> phase 3: unknown locale falls back to English"
UNKNOWN=$(curl -sS -X POST \
    -H "content-type: application/json" \
    -H "accept-language: fr,de;q=0.9" \
    "${BASE}/api/v1/auth/register" \
    -d '{"email":"","password":"","display_name":""}' | extract_msg)
echo "  fr,de -> ${UNKNOWN}"
assert_contains "unknown header -> English catalog" "${UNKNOWN}" "validation"

# ---------------------------------------------------------------
# Phase 4: Override wins for a key that has one
# ---------------------------------------------------------------
echo "==> phase 4: per-tenant override wins"
OVERRIDE_HIT=$(login_err "en" "" | extract_msg)
echo "  en (override) -> ${OVERRIDE_HIT}"
assert_contains "per-tenant override present" "${OVERRIDE_HIT}" "[OVERRIDE]"
# And the same key in another locale still uses the override
# (the override is per-locale, so Thai falls back to the file
# catalog for invalid_credentials).
TH_OVERRIDE=$(login_err "th" "" | extract_msg)
echo "  th (no override) -> ${TH_OVERRIDE}"
assert_not_contains "th does not pick up en override" "${TH_OVERRIDE}" "[OVERRIDE]"
assert_contains "th uses Thai catalog" "${TH_OVERRIDE}" "อีเมลหรือรหัสผ่าน"

# ---------------------------------------------------------------
# Done
# ---------------------------------------------------------------
echo
echo "ALL CHECKS PASSED"
echo "  data dir: ${DATA_DIR}"
echo "  log:      ${LOG}"
