# Kokkeak Helm Chart (T-22)

Production-shape Helm chart for Kokkeak_API.

## Install (staging)

```bash
helm upgrade --install kokkak deploy/helm \
  --namespace kokkak-staging --create-namespace \
  --set image.tag=$(git rev-parse --short HEAD) \
  --values deploy/helm/values.yaml
```

## Override secrets

Never put real secrets in `values.yaml`; mount them via
`external-secrets`, `sealed-secrets`, or `--values` from a
secrets manager. The chart ships with placeholder values that
fail runtime validation in production.

```bash
helm upgrade --install kokkak deploy/helm \
  --set secret.KOKKAK_AUTH__JWT_SECRET="$JWT_SECRET" \
  --set secret.KOKKAK_DATABASE__SQLSERVER_URL="$SQL_URL" \
  --set secret.KOKKAK_REDIS__URL="$REDIS_URL" \
  --set secret.KOKKAK_NATS__URL="$NATS_URL" \
  --set secret.KOKKAK_MONGO__URL="$MONGO_URL"
```

## Resources

| Kind | File | Notes |
|---|---|---|
| ConfigMap | `templates/configmap.yaml` | Non-secret runtime config. |
| Secret | `templates/secret.yaml` | JWT, DB, Redis, NATS, Mongo URLs. |
| ServiceAccount | `templates/serviceaccount.yaml` | Non-root by default. |
| Deployment (api) | `templates/api-deployment.yaml` | 2 replicas, hpa 2-10. |
| Service (api) | `templates/api-service.yaml` | ClusterIP on :80. |
| Deployment (worker) | `templates/worker-deployment.yaml` | 1 replica (HPA off by default). |
| Ingress | `templates/ingress.yaml` | TLS via cert-manager. |
| HPA | `templates/hpa.yaml` | API autoscale 70% CPU. |
| PDB | `templates/pdb.yaml` | `minAvailable: 1` for api. |
| Certificate | `templates/certificate.yaml` | cert-manager issuance. |

## Verify locally

Render without applying:

```bash
helm template kokkak deploy/helm --set image.tag=dev
```

Lint:

```bash
helm lint deploy/helm
```