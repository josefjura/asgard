# Operations

Asgard runs as a single-replica Deployment in the MicroK8s cluster on our VPS, alongside the other apps, at `https://asgard.joseforge.cz`.

## Build and deploy

- A multi-stage `Dockerfile` builds a static musl binary with the `dist` profile (LTO, stripped) and copies it onto `gcr.io/distroless/static-debian12:nonroot`. The image runs as `nonroot` (uid 65532), has no shell, and owns `/data`, where `DATABASE_URL` and `BACKUP_DIR` point by default. A volume mounted at `/data` hides that ownership, so the Deployment sets `securityContext.fsGroup: 65532`.
- Kubernetes manifests live in `deploy/`: Deployment, Service, Ingress, PVC.
- The Deployment uses `replicas: 1` and `strategy: Recreate` (ADR 0004). Probes point at the health endpoints.

### Releases

Nobody tags or bumps the version by hand. `.github/workflows/release.yml` runs on every push to `master`:

1. Each squash-merged PR lands on `master` with its Conventional Commits title. Release Please opens or updates its release PR, which bumps `Cargo.toml` and `Cargo.lock` and adds the `CHANGELOG.md` entry. `feat:` bumps the minor version and `fix:` the patch. Before 1.0, a breaking change also bumps only the minor version.
2. Merging the release PR creates the `vX.Y.Z` tag and the GitHub Release.
3. The same workflow run then builds the image and pushes `ghcr.io/josefjura/asgard:X.Y.Z` and `:latest`. It can't be a separate `on: push: tags` workflow, because a tag pushed with `GITHUB_TOKEN` never triggers another workflow.

To deploy, set the new `X.Y.Z` in the Deployment's image.

Things to know:

- The release PR is opened with `GITHUB_TOKEN`, so CI doesn't run on it. It only changes the version and changelog, and the commit it squashes from was already checked on `master`.
- The repository setting *Actions → General → Allow GitHub Actions to create and approve pull requests* must be on, or Release Please can't open its PR.
- If the image job fails, use *Re-run failed jobs* on that workflow run. It reuses Release Please's outputs from the first attempt, and the tag and Release stay in place. A fresh run wouldn't work, because Release Please reports a release only on the run that creates it.
- Configuration lives in `release-please-config.json` and `.release-please-manifest.json`. The manifest holds the last released version.

### Running the image locally

```sh
docker build -t asgard .
docker run --rm -p 8080:8080 \
  -e PUBLIC_URL=http://localhost:8080 \
  -e VAPID_PRIVATE_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA \
  asgard
curl http://localhost:8080/health/live
curl http://localhost:8080/health/ready
```

The all-zero VAPID key is only good enough to pass startup validation. To keep the database between runs, mount a volume at `/data`.

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

## The first Admin

On a fresh database, create the first Admin's Invite link with the bootstrap subcommand. It runs with the same environment as the server, so in the cluster run it inside the pod:

```sh
kubectl exec deploy/asgard -- /usr/local/bin/asgard bootstrap-admin "Josef"
```

It prints a link that works once, for 24 hours. Opening it registers a passkey and signs the new Member in as the Admin. The subcommand refuses once the Crew has an Admin; it is the only way to create one.

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
