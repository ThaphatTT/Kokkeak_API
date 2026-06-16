#!/bin/bash
set -e
cd "C:\Users\crybo\Desktop\Develop\Kokkeak_API"
export KOKKAK_AUTH__JWT_SECRET=test-secret
export KOKKAK_DATA_DIR__RESET_ON_STARTUP=true
export KOKKAK_SERVER__ADDR=127.0.0.1:18082

# Start the API server in background.
./target/debug/kokkak-api.exe > /tmp/api-m8-m9.log 2>&1 &
API_PID=$!
trap "kill $API_PID 2>/dev/null" EXIT

# Wait for boot.
for i in 1 2 3 4 5 6 7 8; do
  if curl -s -f http://127.0.0.1:18082/healthz > /dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "==== healthz ===="
curl -s http://127.0.0.1:18082/healthz
echo ""

echo "==== register customer + technician ===="
TS=$(date +%s)
CUST_EMAIL="m8m9-cust-$TS@example.com"
TECH_EMAIL="m8m9-tech-$TS@example.com"
CUST_REG=$(curl -s -X POST -H "Content-Type: application/json" -d "{\"email\":\"$CUST_EMAIL\",\"password\":\"supersecret-123\",\"display_name\":\"Cust\",\"role\":\"customer\",\"locale\":\"lo\"}" http://127.0.0.1:18082/api/v1/auth/register)
CUST_TOKEN=$(echo "$CUST_REG" | grep -oE '"access_token":"[^"]+"' | head -1 | sed 's/"access_token":"//' | sed 's/"$//')
CUST_ID=$(echo "$CUST_REG" | grep -oE '"id":"[a-f0-9-]+"' | head -1 | sed 's/"id":"//' | sed 's/"$//')

TECH_REG=$(curl -s -X POST -H "Content-Type: application/json" -d "{\"email\":\"$TECH_EMAIL\",\"password\":\"supersecret-123\",\"display_name\":\"Tech\",\"role\":\"technician\",\"locale\":\"lo\"}" http://127.0.0.1:18082/api/v1/auth/register)
TECH_TOKEN=$(echo "$TECH_REG" | grep -oE '"access_token":"[^"]+"' | head -1 | sed 's/"access_token":"//' | sed 's/"$//')
TECH_ID=$(echo "$TECH_REG" | grep -oE '"id":"[a-f0-9-]+"' | head -1 | sed 's/"id":"//' | sed 's/"$//')

echo "  customer id = $CUST_ID"
echo "  technician id = $TECH_ID"

echo ""
echo "==== POST /api/v1/chat/rooms (customer opens a room) ===="
ROOM=$(curl -s -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $CUST_TOKEN" \
  -d "{\"other_user_id\":\"$TECH_ID\",\"other_role\":\"technician\"}" \
  http://127.0.0.1:18082/api/v1/chat/rooms)
echo "$ROOM" | head -c 350
echo ""
ROOM_ID=$(echo "$ROOM" | grep -oE '"id":"[a-f0-9-]+"' | head -1 | sed 's/"id":"//' | sed 's/"$//')
echo "  room_id = $ROOM_ID"

echo ""
echo "==== POST /api/v1/chat/rooms/:id/messages ===="
curl -s -w "  [HTTP %{http_code}]" -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $CUST_TOKEN" \
  -d '{"body":"ສະບາຍດີ, ຊ່າງ!"}' \
  http://127.0.0.1:18082/api/v1/chat/rooms/$ROOM_ID/messages
echo ""

echo ""
echo "==== GET /api/v1/chat/rooms (technician inbox) ===="
curl -s -w "  [HTTP %{http_code}]" -H "Authorization: Bearer $TECH_TOKEN" http://127.0.0.1:18082/api/v1/chat/rooms
echo ""

echo ""
echo "==== POST /api/v1/orders (M6 create) ===="
ORDER=$(curl -s -w "  [HTTP %{http_code}]" -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $CUST_TOKEN" \
  -d '{"service_code":"ac","description":"AC repair","address":"Vientiane","total":"200.00"}' \
  http://127.0.0.1:18082/api/v1/orders)
echo "$ORDER" | head -c 400
echo ""
ORDER_ID=$(echo "$ORDER" | grep -oE '"id":"[a-f0-9-]+"' | head -1 | sed 's/"id":"//' | sed 's/"$//')

echo ""
echo "==== POST /api/v1/payments (M9) ===="
if [ -n "$ORDER_ID" ]; then
  PAYMENT=$(curl -s -w "  [HTTP %{http_code}]" -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $CUST_TOKEN" \
    -d "{\"order_id\":\"$ORDER_ID\"}" \
    http://127.0.0.1:18082/api/v1/payments)
  echo "$PAYMENT" | head -c 400
  echo ""
  PAYMENT_ID=$(echo "$PAYMENT" | grep -oE '"id":"[a-f0-9-]+"' | head -1 | sed 's/"id":"//' | sed 's/"$//')

  echo ""
  echo "==== POST /api/v1/payments/:id/confirm ===="
  if [ -n "$PAYMENT_ID" ]; then
    curl -s -w "  [HTTP %{http_code}]" -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $CUST_TOKEN" \
      -d '{"gateway_ref":"pi_e2e"}' \
      http://127.0.0.1:18082/api/v1/payments/$PAYMENT_ID/confirm
    echo ""
  fi
fi

echo ""
echo "==== GET /api/v1/payments/me ===="
curl -s -w "  [HTTP %{http_code}]" -H "Authorization: Bearer $CUST_TOKEN" http://127.0.0.1:18082/api/v1/payments/me
echo ""

echo ""
echo "==== ALL M8+M9 DONE ===="
