# Kokkeak Secret Management (T-29)

Production secrets never live in `.env`, the chart's `values.yaml`,
or the container image. They come from a dedicated secret manager
and reach the pods via the External Secrets Operator (ESO) +
Kubernetes Secret indirection.

## Source of truth

| Secret | Source | Rotation | Owner |
|---|---|---|---|
| `KOKKAK_AUTH__JWT_SECRET` | Vault / cloud KMS | 30d | platform |
| `KOKKAK_DATABASE__SQLSERVER_URL` | Vault dynamic creds | 24h | platform |
| `KOKKAK_REDIS__URL` | Vault static | 90d | platform |
| `KOKKAK_NATS__URL` | Vault static | 90d | platform |
| `KOKKAK_MONGO__URL` | Vault static | 90d | platform |
| `KOKKAK_S3__ACCESS_KEY` | cloud IAM | 90d | platform |

## How secrets reach a pod

```
[cloud secret manager / Vault]
            │
            ▼ (refreshInterval)
[ExternalSecret + SecretStore]   ← deploy/helm/templates/externalsecret.yaml
            │
            ▼ (creates / refreshes)
[Kubernetes Secret: kokkak-secret]
            │
            ▼ (envFrom in Deployment)
[container env: KOKKAK_*]
```

The chart's `templates/secret.yaml` only renders when
`externalSecrets.enabled=false`. When ESO is active (recommended
for staging + prod), the ExternalSecret creates the Kubernetes
Secret at the same name.

## Setup checklist (one-time per cluster)

1. **Install ESO.** Helm repo + values:
   ```bash
   helm repo add external-secrets https://charts.external-secrets.io
   helm install external-secrets external-secrets/external-secrets \
     --namespace external-secrets --create-namespace
   ```

2. **Create a SecretStore.** This is the cluster-side handle to
   the upstream secret manager. Example for AWS Secrets Manager:
   ```yaml
   apiVersion: external-secrets.io/v1beta1
   kind: SecretStore
   metadata:
     name: kokkak-aws-sm
     namespace: kokkak-prod
   spec:
     provider:
       aws:
         service: SecretsManager
         region: ap-southeast-1
         auth:
           jwt:
             serviceAccountRef:
               name: eso-sa   # IRSA annotation required
   ```

3. **Pre-create the upstream secrets** in AWS Secrets Manager /
   Vault with the keys the chart expects (see
   `deploy/helm/values.yaml` → `secret:`).

4. **Override the chart's `secret.remoteRefKey` to point at the
   upstream path.** Example:
   ```yaml
   secret:
     KOKKAK_AUTH__JWT_SECRET:
       remoteRefKey: kokkak/prod/jwt-secret
   ```

5. **Verify ESO synced:**
   ```bash
   kubectl get externalsecret -n kokkak-prod
   kubectl get secret kokkak-secret -n kokkak-prod -o yaml
   # Should show a `data` block populated by ESO.
   ```

6. **Restart deployments** so they pick up the new env vars:
   ```bash
   kubectl rollout restart deploy/kokkak-api -n kokkak-prod
   kubectl rollout restart deploy/kokkak-worker -n kokkak-prod
   ```

## Rotating the JWT secret

JWT secret rotation needs **two releases** (otherwise tokens
issued under the old key become invalid mid-flight):

1. **Overlapping keys.** Add the new secret as
   `KOKKAK_AUTH__JWT_SECRET_NEXT` and update `jwt.rs` to verify
   against either. (Planned — not yet implemented. Track in
   `KOKKAK_TASKS_PLAN.md` follow-ups.)
2. **Issue under new key.** Flip the issuer to use the new
   secret. Old tokens still verify until they expire (15m by
   default).
3. **Drop the old key.** Once all old tokens have aged out
   (refresh TTL + safety margin), remove
   `KOKKAK_AUTH__JWT_SECRET_NEXT`.

Until #1 lands, rotation requires a brief invalidation window:
issue a maintenance window, roll the secret, expect ~15m of
forced re-logins. Acceptable for a low-traffic staging env;
**must** be fixed before the first high-traffic prod rollout.

## What NOT to do

- **Don't bake secrets into the image.** Anyone with `docker
  pull` can read them. Mounted env vars / Secret refs only.
- **Don't commit `.env`.** It's gitignored — keep it that way.
  Use `.env.example` for templates.
- **Don't log secrets.** Even at TRACE level. The chart's
  tracing init filters out env values, but custom spans must
  take care not to include `KOKKAK_*` keys.
- **Don't reuse dev secrets in prod.** The `.env.example`
  contains obvious placeholders — production values come from
  the secret manager, not from copy-pasted dev creds.

## Auditing

Quarterly:
- Run `kubectl get externalsecret -A -o yaml` and confirm every
  cluster has a synced ExternalSecret per namespace.
- Verify no Secret in any `kokkak-*` namespace was created by a
  human (`kubectl get secret -n kokkak-prod -o json | jq
  '.items[].metadata.annotations'`) — ESO adds a specific
  annotation; absence means manual.
- Rotate the JWT signing key on the published cadence even if
  no incident has occurred.

## References

- [External Secrets Operator docs](https://external-secrets.io/)
- [AWS Secrets Manager + ESO](https://external-secrets.io/latest/provider/aws-secrets-manager/)
- [HashiCorp Vault + ESO](https://external-secrets.io/latest/provider/hashicorp-vault/)
- `deploy/helm/values.yaml` — `externalSecrets` block