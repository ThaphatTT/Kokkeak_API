// Kokkeak load test — login flow (T-26, k6).
//
// Exercises the JWT login endpoint under sustained load. The
// shape mirrors the mobile app's first-launch path:
//
//   1. POST /api/v1/auth/login   → get access + refresh tokens
//   2. GET  /api/v1/users/me     → confirm the token is valid
//
// Targets (from KOKKAK_TASKS_PLAN.md T-26):
//   - RPS:  500 sustained, 1000 burst
//   - p99:  < 500ms
//   - 5xx:  < 0.1%
//
// Run:
//   k6 run --out json=results.json scripts/load/login.js
//   k6 run -e BASE_URL=https://staging.kokkak.example scripts/load/login.js

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

// ---- Custom metrics (surface as separate lines in the summary) ----
const loginFailRate = new Rate('kokkak_login_failures');
const loginLatency = new Trend('kokkak_login_latency', true);

export const options = {
  scenarios: {
    // Steady load — typical "normal" traffic.
    sustained: {
      executor: 'constant-arrival-rate',
      rate: 500,                    // 500 iterations per `timeUnit`
      timeUnit: '1s',
      duration: '5m',
      preAllocatedVUs: 100,
      maxVUs: 500,
      tags: { scenario: 'sustained' },
    },
    // Spike — 2x RPS for 30s to catch connection-pool exhaustion.
    burst: {
      executor: 'constant-arrival-rate',
      rate: 1000,
      timeUnit: '1s',
      duration: '30s',
      preAllocatedVUs: 200,
      maxVUs: 1000,
      startTime: '5m30s',           // after sustained finishes
      tags: { scenario: 'burst' },
    },
  },
  thresholds: {
    // Hard pass/fail for CI gating.
    http_req_duration: ['p(99)<500'],   // 99% of requests < 500ms
    http_req_failed:   ['rate<0.001'],  // 0.1% error budget
    kokkak_login_failures: ['rate<0.01'],
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';

// ---- Seed users ----
// `seed.json` lives next to this script and contains an array
// of {username, password}. Generate with:
//   jq '[.users[] | {username, password}]' data/json_db/user.json > scripts/load/seed.json
const SEED = JSON.parse(open('./seed.json'));

export default function () {
  // Pick a random seeded user. Round-robin is fine — k6 VUs
  // are independent and any VU may grab any user.
  const user = SEED[Math.floor(Math.random() * SEED.length)];

  const loginRes = http.post(
    `${BASE_URL}/api/v1/auth/login`,
    JSON.stringify({
      username: user.username,
      password: user.password,
    }),
    { headers: { 'Content-Type': 'application/json' } },
  );

  loginLatency.add(loginRes.timings.duration);
  const loginOk = check(loginRes, {
    'login status is 200': (r) => r.status === 200,
    'login has access_token': (r) => r.json('data.access_token') !== undefined,
  });
  loginFailRate.add(!loginOk);
  if (!loginOk) return;

  const accessToken = loginRes.json('data.access_token');

  // Confirm the token works.
  const meRes = http.get(`${BASE_URL}/api/v1/users/me`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  check(meRes, {
    'me status is 200': (r) => r.status === 200,
    'me returns expected user_guid': (r) => r.json('data.user_guid') !== undefined,
  });

  // Cooldown between iterations per VU. Picked to match the
  // arrival-rate target without stacking requests in a single
  // tokio worker.
  sleep(0.1);
}
