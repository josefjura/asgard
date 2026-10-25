# SQLite in a single replica

Asgard stores everything in one SQLite file (via sqlx, with compile-time-checked queries) on a persistent volume, and runs as exactly one pod with `strategy: Recreate`, so two processes never write to the file. A handful of users don't need a database server, and SQLite gives every test a fresh database in milliseconds. Backups must use `VACUUM INTO`, never a copy of the live file.

## Considered Options

- **Postgres:** a database server for three users.
- **The existing MySQL 8 instance on the VPS:** tests and local development would need a running server, and a shared instance ties Asgard to another app's upgrades and outages. It would only pay off with several replicas or zero-downtime deploys, which v1 doesn't need. Switching later means porting dialect-specific queries: hours, not weeks.

## Consequences

- Each deploy has a few seconds of downtime.
