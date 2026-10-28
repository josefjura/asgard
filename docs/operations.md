# Operations

Asgard runs as a single-replica Deployment in the MicroK8s cluster on our VPS, alongside the other apps, at `https://asgard.joseforge.cz`.

## Build and deploy

- A multi-stage `Dockerfile` builds a static musl binary with the `dist` profile (LTO, stripped) and copies it onto `gcr.io/distroless/static-debian12:nonroot`. The image runs as `nonroot` (uid 65532), has no shell, and owns `/data`, where `DATABASE_URL` and `BACKUP_DIR` point by default. A volume mounted at `/data` hides that ownership, so the Deployment sets `securityContext.fsGroup: 65532`.
- Kubernetes manifests live in `deploy/` and are applied with Kustomize (`kubectl apply -k deploy/`) into the namespace `asgard`: Namespace, PVC, Deployment, Service, Ingress. `secret.example.yaml` lists the Secret's keys only; it is not part of the Kustomization, and the real Secret is created by hand.
- The Deployment uses `replicas: 1` and `strategy: Recreate` (ADR 0004). Liveness checks `/health/live`, readiness `/health/ready`. The root filesystem is read-only; only `/data` (the PVC) and `/tmp` (an `emptyDir` for SQLite's temporary files) are writable.

### Releases

Nobody tags or bumps the version by hand. `.github/workflows/release.yml` runs on every push to `master`:

1. Each squash-merged PR lands on `master` with its Conventional Commits title. Release Please opens or updates its release PR, which bumps `Cargo.toml` and `Cargo.lock` and adds the `CHANGELOG.md` entry. `feat:` bumps the minor version and `fix:` the patch. Before 1.0, a breaking change also bumps only the minor version.
2. Merging the release PR creates the `vX.Y.Z` tag and the GitHub Release.
3. The same workflow run then builds the image and pushes `ghcr.io/josefjura/asgard:X.Y.Z` and `:latest`. It can't be a separate `on: push: tags` workflow, because a tag pushed with `GITHUB_TOKEN` never triggers another workflow.

To deploy, set the new `X.Y.Z` in the Deployment's image (see [Upgrade](#upgrade)).

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

### VAPID key

`VAPID_PRIVATE_KEY` is a raw P-256 private key, base64url without padding (43 characters). Generate one on any machine with OpenSSL:

```sh
openssl ecparam -genkey -name prime256v1 -noout \
  | openssl ec -outform DER 2>/dev/null \
  | tail -c +8 | head -c 32 \
  | base64 | tr '+/' '-_' | tr -d '=\n'
```

The DER encoding puts the 32-byte private key right after a 7-byte header, which is what `tail` and `head` cut out.

Generate it once and keep a copy in the password manager. Every push subscription is tied to this key: a new key silently stops notifications until each Member subscribes again.

### First deploy

Run these from a clone of the repository, with `kubectl` pointed at the MicroK8s cluster.

1. **Check the prerequisites.**
   - The image tag in `deploy/deployment.yaml` has been released: `ghcr.io/josefjura/asgard:X.Y.Z` exists.
   - The package is public (*GitHub → Packages → asgard → Package settings → Change visibility*), because the Deployment has no pull secret.
   - `asgard.joseforge.cz` resolves to the cluster's ingress IP: `dig +short asgard.joseforge.cz`.
2. **Create the namespace and the Secret.** Paste the key from the password manager (see [VAPID key](#vapid-key)) when `read` asks, so it never lands in the shell history:

   ```sh
   kubectl apply -f deploy/namespace.yaml
   read -rs VAPID_PRIVATE_KEY
   kubectl -n asgard create secret generic asgard-secrets \
     --from-literal=VAPID_PRIVATE_KEY="$VAPID_PRIVATE_KEY"
   unset VAPID_PRIVATE_KEY
   ```

3. **Apply the manifests and wait for the pod:**

   ```sh
   kubectl apply -k deploy/
   kubectl -n asgard rollout status deploy/asgard
   ```

   If the pod crashes, `kubectl -n asgard logs deploy/asgard` names the missing or invalid variable.
4. **Wait for the certificate.** cert-manager requests it when the Ingress appears; `READY` turns `True` within a minute or two:

   ```sh
   kubectl -n asgard get certificate asgard-prod-tls
   ```

5. **Check it from outside.** This must print `ok` without `-k`, which proves the Let's Encrypt certificate is valid:

   ```sh
   curl https://asgard.joseforge.cz/health/ready
   ```

6. **Create the first Admin** as described in [The first Admin](#the-first-admin), and sign in.
7. **Check that data survives a restart.** Delete the pod, wait for the new one, and sign in again: the Admin and the passkey are still there.

   ```sh
   kubectl -n asgard delete pod -l app=asgard
   kubectl -n asgard rollout status deploy/asgard
   ```

### Upgrade

1. Merge the release PR and wait for the release workflow's image job to push `ghcr.io/josefjura/asgard:X.Y.Z`.
2. Set the image tag in `deploy/deployment.yaml` to `X.Y.Z` and merge that change, so `master` always shows what is running.
3. Apply it and wait:

   ```sh
   kubectl apply -k deploy/
   kubectl -n asgard rollout status deploy/asgard
   curl https://asgard.joseforge.cz/health/ready
   ```

`Recreate` stops the old pod before the new one starts, so Asgard is down for a few seconds. Migrations run when the new pod starts. Watch it with `kubectl -n asgard get pods -w`: the old pod is `Terminating` and gone before the new one is `ContainerCreating`.

**Rolling back:** set the previous tag and apply again. That works only if the newer release added no migration: an older binary refuses to start on a database with a migration it doesn't know. In that case, restore the last nightly snapshot from before the upgrade (see [Backups and restore](#backups-and-restore)) together with the older tag.

### Volume permissions

The image runs as uid 65532, and the Deployment sets `fsGroup: 65532` so the pod can write to `/data`. MicroK8s's hostpath-storage doesn't apply `fsGroup`, but it creates each volume's directory world-writable, so the pod can write anyway. If the pod logs a permission error on `/data/asgard.db` with another storage class, fix the ownership of the volume's directory on the host (`chown -R 65532:65532 <path>`).

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
kubectl -n asgard exec deploy/asgard -- /usr/local/bin/asgard bootstrap-admin "Josef"
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
