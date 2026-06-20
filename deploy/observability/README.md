# Kokkeak Observability Stack (T-25)

Files here define what "the service is working" looks like in
production. They are deployed alongside Prometheus + Grafana,
typically via kube-prometheus-stack.

## Layout

```
deploy/observability/
├── README.md                       # this file
├── slo.md                          # SLO/SLI source of truth
├── grafana/
│   └── kokkak-api-dashboard.json   # main API overview dashboard
└── prometheus/
    └── alerts.yml                  # PrometheusRule CRD manifest
```

## Deploy

### Grafana dashboard

```bash
# Upload via API (uses Grafana service account token)
curl -X POST -H "Authorization: Bearer $GRAFANA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @deploy/observability/grafana/kokkak-api-dashboard.json \
  "$GRAFANA_URL/api/dashboards/db"
```

Or import through the UI:
**Dashboards → Import → Upload JSON file → pick this file**.

### Prometheus alerts

```bash
kubectl apply -f deploy/observability/prometheus/alerts.yml
```

Requires the `prometheusrules.monitoring.coreos.com` CRD
(installed by kube-prometheus-stack).

## Adding new alerts

1. Add the rule to `alerts.yml` in the matching group.
2. Add the metric to the dashboard if it's operator-facing.
3. Update `slo.md` if the alert fires on an SLO breach.
4. Link the runbook URL in the alert annotation.

## Adding new panels

1. Edit the JSON in `grafana/` — keep `uid` stable so existing
   links keep working.
2. Re-import (the API call above replaces by `uid`).

## Conventions

- **Namespace.** All metrics queries assume `namespace="kokkak-prod"`.
  Override via the Grafana variable for other environments.
- **Buckets.** Histogram buckets live in
  `crates/common/src/telemetry.rs`. Don't change them without
  reviewing the SLO docs — bucket boundaries affect how
  `histogram_quantile()` interpolates.
- **Severity.** `info` / `warning` / `critical` map to PagerDuty
  routing rules — don't reuse severities for different meanings.