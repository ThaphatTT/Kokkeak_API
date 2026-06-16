#!/bin/bash
set -e
cd "C:\Users\crybo\Desktop\Develop\Kokkeak_API"
export KOKKAK_AUTH__JWT_SECRET=dev-secret-123
export KOKKAK_DATA_DIR__RESET_ON_STARTUP=true
export KOKKAK_SERVER__ADDR=127.0.0.1:18080

# Start the server in background (use pre-built debug binary)
nohup ./target/debug/kokkak-api.exe > /tmp/kokkak-server.log 2>&1 &
SERVER_PID=$!
trap "kill $SERVER_PID 2>/dev/null" EXIT

# Wait for boot
for i in 1 2 3 4 5 6 7 8 9 10; do
  if curl -s -f http://127.0.0.1:18080/healthz > /dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "==== healthz ===="
curl -s http://127.0.0.1:18080/healthz
echo ""
echo "==== readyz ===="
curl -s http://127.0.0.1:18080/readyz
echo ""
echo "==== register ===="
TS=$(date +%s)
EMAIL="test-$TS@example.com"
echo "email=$EMAIL"
REG=$(curl -s -X POST -H "Content-Type: application/json" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"supersecret-123\",\"display_name\":\"E2E\",\"role\":\"customer\",\"locale\":\"lo\"}" \
  http://127.0.0.1:18080/api/v1/auth/register)
echo "$REG" | head -c 500
echo ""
TOKEN=$(echo "$REG" | grep -oE '"access_token":"[^"]+"' | head -1 | sed 's/"access_token":"//' | sed 's/"$//')
echo "token-prefix=${TOKEN:0:40}..."

echo ""
echo "==== /users/me ===="
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18080/api/v1/users/me
echo ""

echo ""
echo "==== /api/v1/catalog/services ===="
curl -s http://127.0.0.1:18080/api/v1/catalog/services
echo ""

echo ""
echo "==== /api/v1/orders/me (auth required) ===="
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18080/api/v1/orders/me
echo ""

echo ""
echo "==== login (correct) ===="
curl -s -X POST -H "Content-Type: application/json" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"supersecret-123\",\"scope\":\"mobile\"}" \
  http://127.0.0.1:18080/api/v1/auth/login | head -c 300
echo ""

echo ""
echo "==== login (wrong pwd) ===="
curl -s -w "  [HTTP %{http_code}]" -X POST -H "Content-Type: application/json" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"WRONG\"}" \
  http://127.0.0.1:18080/api/v1/auth/login
echo ""

echo ""
echo "==== /users/me (no token) ===="
curl -s -w "  [HTTP %{http_code}]" http://127.0.0.1:18080/api/v1/users/me
echo ""

echo ""
echo "==== register (duplicate) ===="
curl -s -w "  [HTTP %{http_code}]" -X POST -H "Content-Type: application/json" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"supersecret-123\",\"display_name\":\"dup\"}" \
  http://127.0.0.1:18080/api/v1/auth/register | head -c 200
echo ""

echo ""
echo "==== /metrics (first 5 lines) ===="
curl -s http://127.0.0.1:18080/metrics | head -5

echo ""
echo "==== ALL DONE ===="
