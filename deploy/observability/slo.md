# Kokkeak API — SLO / SLI Definitions (T-25)

This document is the source of truth for what "the service is
working" means. Alerts in `prometheus/alerts.yml` reference these
budgets; dashboards in `grafana/` graph the SLIs.

## Service overview

Kokkeak_API serves three consumer types:

| Consumer | Surface | Criticality |
|---|---|---|
| Customer mobile app | `/api/v1/customer/*` | Revenue path |
| Technician mobile app | `/api/v1/technician/*` | Revenue path |
| Admin web console | `/api/v1/admin/*` | Operational |

Anything outside `/api/v1/` (docs, metrics, healthz, readyz) is
**not** counted toward SLO — it's infrastructure, not user value.

## SLIs (Service Level Indicators)

### 1. Availability

```
availability = good_events / total_events
good_events   = http_requests_total{status!~"5.."}
total_events  = http_requests_total
```

Counted over a **30-day** rolling window, excluding `/healthz`,
`/readyz`, `/metrics`, `/api/docs`, `/api/error-codes.json`.

### 2. Latency

```
latency_p99 = histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))
```

Counted over the same 30-day window. Currently we don't have a
labelled latency metric per route; until then, the global p99 is
the proxy. Once T-25 dashboards show route breakdowns, we can
split budgets.

## SLOs (Service Level Objectives)

| SLI | Budget | Window | Error budget / 30d |
|---|---|---|---|
| Availability | **99.9%** | 30d rolling | 43m 12s downtime |
| Latency p99 | **< 500ms** | 30d rolling | n/a (target, not ratio) |

### Why these numbers?

- 99.9% matches typical B2C marketplace competitors in the
  region. Going higher (99.99%) costs 10x in infra with no
  observable business lift at our current scale.
- p99 < 500ms keeps the mobile UX snappy on 4G (round trip
  budget ≈ 800ms including TLS handshake + render).

## Error budget policy

When the 30-day error budget is **>50% consumed**, freeze
non-critical changes (no rollouts except security patches).
When **>75% consumed**, all non-emergency deploys halt; on-call
focuses on reliability work. At **100%**, the next incident
triggers a formal postmortem + a published RCA.

This policy is in `AGENTS.md` §20 (TODO: link when added) and is
enforced manually until we wire the budget counter into CI.

## Measurement notes

- **Buckets.** The Prometheus histogram uses the
  `metrics-exporter-prometheus` default buckets in
  `crates/common/src/telemetry.rs`. The `0.5` bucket sits
  exactly on our SLO; consider adding finer-grained buckets
  (`0.1, 0.2, 0.3, 0.4, 0.5`) if the SLO needs adjustment.
- **Noise.** Read-only admin endpoints (e.g. `/admin/audit/*`)
  have inherently high p99. Exclude them from the SLO once
  per-route labels are added.
- **Cold start.** First-request latency after a rolling restart
  skews p99. The `start_period` in the readiness probe handles
  this — `/readyz` returns 503 until the pod is warm, so the LB
  doesn't route traffic to a cold pod.

## Updating this document

SLO changes are a cross-team decision. Open an ADR under
`docs/adr/` and tag `@kokkak/platform` + `@kokkak/backend-core`.
A change here implies changing `alerts.yml` and the Grafana
panels that reference the budget.