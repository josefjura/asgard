# Asgard

A phone-first PWA where a three-person home-brewery Crew tracks shared money, a Shopping list, Reminders and Quotes. Vocabulary is in `CONTEXT.md`, architectural decisions in `docs/adr/`, deployment and operations in `docs/operations.md`.

Most code is written by Claude and reviewed by Josef, so prefer choices the compiler can check.

## Conventions

### Stack

Rust (edition 2024), axum + tower-http, Maud templates, htmx (vendored, served by us), SQLite via sqlx with compile-time-checked queries and migrations, tokio, `webauthn-rs`, `web-push` (VAPID). Read ADR 0003 and 0004 before proposing alternatives.

### Code structure

One crate, thin routes:

| Module | Holds | Rule |
| --- | --- | --- |
| `domain/` | Services and types: finance, shopping, reminders, quotes, members | No axum or Maud imports; unit-testable on its own |
| `web/` | Routes, extractors, forms, htmx helpers | Parse input, call a service, render a view |
| `views/` | Maud components and pages | Pure functions from data to HTML, no DB access |
| `tasks/` | Background loops (reminder ticker, notifier, nightly backup) | Depend on domain services only |
| `db.rs`, `config.rs` | Pool setup, migrations, env config | Config validated once at startup |

- **State:** services are built once at startup and held in an `AppState`. Routes extract what they need; no lazy getters.
- **Access:** `Member` and `Admin` extractors. A route's signature shows its access level; no handler checks sessions by hand.
- **Errors:** one `AppError` implementing `IntoResponse`. Handlers return `Result<_, AppError>` and use `?`.
- **Money:** integer whole crowns (CZK only), never floats.
- **Time:** stored as UTC, entered and displayed in `Europe/Prague`.
- **Events:** domain services emit events onto a tokio channel; a notifier task turns them into Web Push, so a slow push never delays a request.
- **UI language:** English.

### Frontend

- The server renders every page; htmx swaps fragments. No client-side rendering, npm or bundler.
- JavaScript only as small hand-written ES modules for browser APIs: passkey ceremonies, push subscription, service worker registration.
- One static, mobile-first stylesheet served with a content-hash URL.
- PWA: `manifest.webmanifest`, icons, and a service worker that caches static assets and shows an offline page. Data pages always need the network.
- Web components only for an isolated widget with real client state, as a standalone custom element the server renders as a tag. Nothing in v1 needs one.

### Testing

Every feature ships with tests.

| Layer | Tool | Covers |
| --- | --- | --- |
| Domain | `#[test]` / `#[tokio::test]` | Shares, Balances, Settlements, reminder scheduling, validation |
| HTTP | axum `Router` via `tower::ServiceExt` with `#[sqlx::test]` | Routes, extractors, status codes, htmx fragments, Origin (CSRF) checks |
| Components | `insta` snapshots of Maud functions | Markup changes as reviewable diffs |
| Visual | `/dev/gallery`, debug builds only | Every component with fixtures in all states, at phone and desktop width |

Check UI changes in the gallery before handing them over.

### CI

`cargo fmt --check`, `cargo clippy -- -D warnings` (pedantic + nursery) and `cargo test` on every push.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on josefjura/asgard, using the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the five default triage labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
