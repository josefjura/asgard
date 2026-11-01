# Asgard

The private app of the three-person crew behind the Runa home brewery. It keeps the Crew's shared money (Expenses, Payments, Balances and Settlements), a Shopping list, Reminders and Quotes in one place, and runs at `https://asgard.joseforge.cz` for Members only.

It is a phone-first PWA: the server renders every page with Maud, htmx swaps fragments, and Web Push delivers Reminders and changes. Members sign in with passkeys; the only way in is an Invite link.

## Stack

Rust (edition 2024), axum, Maud, htmx, SQLite via sqlx, `webauthn-rs` and `web-push`. One binary, one SQLite file, one replica. The reasons are in `docs/adr/`.

## Running it locally

You need the Rust toolchain pinned in `rust-toolchain.toml` (rustup picks it up). On Windows, the vendored OpenSSL also needs Strawberry Perl: set `OPENSSL_SRC_PERL` to its `perl.exe`.

```sh
export DATABASE_URL=sqlite:target/asgard-dev.db
export PUBLIC_URL=http://localhost:8080
export VAPID_PRIVATE_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
cargo run
```

The all-zero VAPID key only passes startup validation; generate a real one (see `docs/operations.md`) to try push notifications. Browsers allow passkeys on `http://localhost`, so sign-in works without HTTPS. Migrations run on start.

To create the first Admin on a fresh database, run the bootstrap subcommand with the same environment and open the link it prints:

```sh
cargo run -- bootstrap-admin "Josef"
```

Debug builds also serve `/dev/gallery`, which shows every component with fixtures in all its states.

## Development

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

CI runs the same three on every push. Queries are checked at compile time against the committed `.sqlx/` data; after changing a query or migration, regenerate it as described in `CLAUDE.md`.

Changes go through a short branch and a squash-merged PR into `master`, with a Conventional Commits title. Release Please turns those titles into versions and the changelog, so nobody tags by hand.

## Documentation

| File | What it covers |
| --- | --- |
| `CONTEXT.md` | The Crew's vocabulary: what an Expense, Share, Balance or Settlement means |
| `docs/adr/` | Architectural decisions and why they were made |
| `docs/operations.md` | Build, release, deploy, configuration, backups |
| `CLAUDE.md` | Code structure and conventions |
| `CHANGELOG.md` | What each release changed |
