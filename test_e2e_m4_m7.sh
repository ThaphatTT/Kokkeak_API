#!/bin/bash
set -e
cd "C:\Users\crybo\Desktop\Develop\Kokkeak_API"
export KOKKAK_AUTH__JWT_SECRET=test-secret
export KOKKAK_DATA_DIR__RESET_ON_STARTUP=true
export KOKKAK_SERVER__ADDR=127.0.0.1:18081

# Start the API server in background.
./target/debug/kokkak-api.exe > /tmp/api-final.log 2>&1 &
API_PID=$!
trap "kill $API_PID 2>/dev/null" EXIT

# Wait for boot.
for i in 1 2 3 4 5 6 7 8; do
  if curl -s -f http://127.0.0.1:18081/healthz > /dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "==== healthz ===="
curl -s http://127.0.0.1:18081/healthz
echo ""
echo "==== readyz ===="
curl -s http://127.0.0.1:18081/readyz
echo ""
echo "==== register ===="
TS=$(date +%s)
EMAIL="final-$TS@example.com"
REG=$(curl -s -X POST -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"supersecret-123\",\"display_name\":\"Final\",\"role\":\"customer\"}" http://127.0.0.1:18081/api/v1/auth/register)
echo "$REG" | head -c 350
echo ""
TOKEN=$(echo "$REG" | grep -oE '"access_token":"[^"]+"' | head -1 | sed 's/"access_token":"//' | sed 's/"$//')

echo ""
echo "==== POST /api/v1/orders (M6 create) ===="
curl -s -w "  [HTTP %{http_code}]" -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" \
  -d '{"service_code":"ac-not-cooling","description":"แอร์ไม่เย็น","address":"ບ້ານຂອງຄົວ","total":"150000.00"}' \
  http://127.0.0.1:18081/api/v1/orders
echo ""

echo ""
echo "==== GET /api/v1/orders/me ===="
curl -s -w "  [HTTP %{http_code}]" -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18081/api/v1/orders/me
echo ""

echo ""
echo "==== GET /api/v1/users/me (with lo Accept-Language) ===="
curl -s -w "  [HTTP %{http_code}]" -H "Authorization: Bearer $TOKEN" -H "Accept-Language: lo" http://127.0.0.1:18081/api/v1/users/me
echo ""

echo ""
echo "==== /metrics (last 5 lines) ===="
curl -s http://127.0.0.1:18081/metrics | tail -5
echo ""

echo ""
echo "==== ALL DONE ===="
