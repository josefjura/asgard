# Operations

Asgard runs as a single-replica Deployment in the MicroK8s cluster on our VPS, alongside the other apps, at `https://asgard.joseforge.cz`.

## Build and deploy

- A multi-stage Dockerfile builds a static release binary (LTO, stripped).
- Pushing a `v*.*.*` git tag publishes the image to GitHub Container Registry.
- Kubernetes manifests live in `deploy/`: Deployment, Service, Ingress, PVC.
- The Deployment uses `replicas: 1` and `strategy: Recreate` (ADR 0004). Probes point at the health endpoints.

## Ingress and HTTPS

HTTPS is required for passkeys, Web Push and service workers.

- **Controller:** MicroK8s's ingress addon, ingress-nginx (`nginx-ingress-microk8s-controller` in namespace `ingress`). IngressClasses `nginx` and `public` both point to it. MetalLB provides LoadBalancer IPs.
- **Certificates:** the cert-manager addon (namespace `cert-manager`) with the ClusterIssuer `letsencrypt-prod`, using Let's Encrypt production and the HTTP-01 challenge. Certificates renew automatically.
- **Asgard's Ingress**, like every other app's, needs:
  - annotation `cert-manager.io/cluster-issuer: "letsencrypt-prod"`
  - `ingressClassName: nginx`
  - a `tls:` block for `asgard.joseforge.cz` with `secretName: asgard-prod-tls`
- **Caveat:** the issuer's challenge ingress uses class `public` while apps use `nginx`. This works only because both classes go to the same controller. If a second controller is ever added, switch the solver to `nginx`.

## Configuration

Environment variables, validated at startup. Secrets come from a Kubernetes Secret.

| Variable | Purpose |
| --- | --- |
| `DATABASE_URL` | SQLite file on the PVC |
| `PUBLIC_URL` | `https://asgard.joseforge.cz`; also the WebAuthn relying party, never change it (ADR 0005) |
| `VAPID_PRIVATE_KEY` | Web Push signing key |
| `BACKUP_DIR` | Where nightly snapshots are written |
| `HOST` / `PORT` | Listen address |
| `RUST_LOG` | Log filter |

## Health

- `/health/live`: the process is up.
- `/health/ready`: the database answers.

## Backups and restore

- Asgard writes a consistent nightly snapshot with `VACUUM INTO` to `BACKUP_DIR`. Never copy the live file; that can produce a corrupt backup.
- The VPS's existing daily job uploads the snapshot to OneDrive and keeps the last 14.
- To restore, replace the database file on the volume with a snapshot and restart the pod.

## Heimdall import

A one-off subcommand: `asgard import-heimdall <path-to-heimdall.db>`.

- Imports Expenses, Payments, Reminders and Quotes. Heimdall never stored todos.
- Members are matched by name and Discord ids are dropped. All three imported Members are in the Crew from the start.
- Amounts become whole crowns, and each Expense date is the Heimdall creation time. Imported entries have no Logger, and imported Quotes have no Added by.
- It prints totals and per-Member Balances next to Heimdall's. The import is verified when the expense and payment totals match exactly and every Balance matches to within 1 Kč.
- Cutover: the Crew stops entering data in Heimdall for the day of the switch. After verification, Heimdall and its bot are switched off.
