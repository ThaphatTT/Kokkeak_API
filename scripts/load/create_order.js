// Kokkeak load test — create-order flow (T-26).
//
// Exercises the order-create path: register a fresh customer,
// log in, then create an order. Heavy DB writes + idempotency
// cache hits — different pressure profile than `login.js`.
//
// Run:
//   k6 run scripts/load/create_order.js
//
// Notes:
//   - Each iteration creates a unique customer (timestamp-based
//     username) so we don't churn the same row.
//   - Idempotency-Key header is set per request — exercises the
//     T-15 middleware.
//   - Realistic mobile payload: lat/long for Vientiane, USD-ish
//     total amount.

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter } from 'k6/metrics';

const orderCreated = new Counter('kokkak_orders_created');
const idempotencyHits = new Counter('kokkak_idempotency_hits');

export const options = {
  scenarios: {
    sustained: {
      executor: 'constant-arrival-rate',
      rate: 100,        // lower than login because DB-bound
      timeUnit: '1s',
      duration: '3m',
      preAllocatedVUs: 50,
      maxVUs: 200,
      tags: { scenario: 'sustained' },
    },
  },
  thresholds: {
    http_req_duration: ['p(95)<1000'],   // order create slower than login
    http_req_failed:   ['rate<0.01'],
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';

export default function () {
  // ---- Register + login as a fresh user ----
  const ts = Date.now();
  const username = `loadtest_${__VU}_${ts}_${Math.floor(Math.random() * 1e6)}`;
  const password = 'LoadTest!Passw0rd';

  const regRes = http.post(
    `${BASE_URL}/api/v1/auth/register`,
    JSON.stringify({
      username,
      password,
      display_name: `Load Test ${username}`,
      locale: 'lo',
    }),
    { headers: { 'Content-Type': 'application/json' } },
  );
  if (regRes.status !== 201) {
    check(regRes, { 'register succeeded': () => false });
    return;
  }

  const loginRes = http.post(
    `${BASE_URL}/api/v1/auth/login`,
    JSON.stringify({ username, password }),
    { headers: { 'Content-Type': 'application/json' } },
  );
  const accessToken = loginRes.json('data.access_token');
  if (!accessToken) {
    check(loginRes, { 'login succeeded': () => false });
    return;
  }

  // ---- Create an order ----
  const idempotencyKey = `${username}-${ts}`;
  const orderRes = http.post(
    `${BASE_URL}/api/v1/orders`,
    JSON.stringify({
      service_id: 'svc-electrical-outlet',
      address: 'Samsenthai Road, Vientiane',
      lat: 17.9757,
      lng: 102.6331,
      scheduled_at: null,
      notes: 'Load test order — safe to ignore.',
    }),
    {
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${accessToken}`,
        'Idempotency-Key': idempotencyKey,
      },
    },
  );

  const ok = check(orderRes, {
    'order create status is 201': (r) => r.status === 201,
    'order has order_guid': (r) => r.json('data.order_guid') !== undefined,
  });
  if (ok) orderCreated.add(1);

  // Replay the same request with the same Idempotency-Key —
  // should return the same response (T-15 middleware short-
  // circuits before hitting the use case).
  const replayRes = http.post(
    `${BASE_URL}/api/v1/orders`,
    JSON.stringify({
      service_id: 'svc-electrical-outlet',
      address: 'Samsenthai Road, Vientiane',
      lat: 17.9757,
      lng: 102.6331,
      scheduled_at: null,
      notes: 'Load test order — safe to ignore.',
    }),
    {
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${accessToken}`,
        'Idempotency-Key': idempotencyKey,
      },
    },
  );
  if (replayRes.headers['Idempotency-Replayed'] === 'true') {
    idempotencyHits.add(1);
  }

  sleep(0.5);
}
