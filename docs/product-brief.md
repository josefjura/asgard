# Asgard — Product Brief

Oct 25, 2026 · Josef Jura · [Live doc](https://claude.ai/artifact/K9UA8A8hnxTsmQoxvG9xVa)

## Context

We are leaving Discord and building a new web app from scratch, not adapting Heimdall. The new app is called Asgard.

- **Discord fatigue:** its behaviour and pricing no longer suit us, so our chat moves to another tool.
- **The bot loses its reason to exist:** it mainly served shenanigans (`/viking`, `/quote`) and finance entry. Once finances live only on the web, nothing needs to run inside the chat.
- **Heimdall is built around Discord:** login, user identity (`discord_id`), notifications and todos all depend on it. Bending it costs more than starting over, and usage will change a lot.

Heimdall stays useful as a reference for the finance logic (expenses, payments, debts ledger) and as the source of existing data.

## Vision and goals

A phone-first app where the brewery crew keeps shared money, tasks and reminders in one place, with no dependency on any chat platform.

**Goals**

1. Log an expense or payment from a phone in under 15 seconds.
2. Everyone can see at any time who owes whom, and how much.
3. Reminders and important events reach people as phone notifications.
4. Brewery todos are tracked in the app, not lost in chat history.
5. One login that never depends on a third-party platform.

**Non-goals**

- No messaging, channels or threads. Conversation lives in the chat tool.
- No bot in the chat.
- No offline editing in v1. The app shell loads offline, but data needs a connection.

## Users

A small, closed group: the brewery crew (Heimdall has 3 member slots today), invite-only, with no public sign-up.

| Role | Who | Typical use |
| --- | --- | --- |
| Member | Everyone in the crew | Log expenses and payments, check balances, add and tick off todos, set reminders |
| Admin | Josef Jura | Invite and remove members, correct anyone's entries |

Most use is on a phone, in short sessions, often right after buying something.

## Scope v1

v1 is the switch to the app: it carries over what the crew uses today and adds no new features. Quotes stay; the other fun extras are dropped.

| Feature | What it does | In Heimdall today | Priority |
| --- | --- | --- | --- |
| Expenses | Log what someone paid for the brewery, always split equally across the whole crew; edit and delete | Bot + web | Must (v1) |
| Payments | Log money sent between members, including on behalf of others | Bot + web | Must (v1) |
| Balances and debts | Who owes whom, net per person | Web dashboard + `/stats` | Must (v1) |
| Finance history | Filterable list and totals over time | `/stats` only | Should |
| Todos | Title, description, assignee, open/done | Discord forum post | Must (v1) |
| Reminders | Date-time + text, sent as a push notification | Bot + web | Must (v1) |
| Members and invites | Invite by link, manage the member list | `/assign` slots | Must (v1) |
| Quotes | The crew's quote collection | Bot + web | Must (v1) |

## Communication

The app and the chat tool are independent. The app works fully without any chat integration, so choosing the chat tool doesn't block the product.

- **Notifications** come from the app itself, as Web Push to installed phones. There is no bot posting into the chat.
- **Optional later:** a one-way webhook that posts events (new todo, big expense) into a chat channel. It's a nice-to-have, not v1.
- **Linking:** a todo can hold a link to its chat thread, so discussion and tracking stay connected without an integration.

Chat tool: a Telegram group with Topics is the front-runner, but not final. Alternatives are Zulip and Mattermost.

## Platform and tech direction

The proposal is an installable PWA on a single self-hosted server. It keeps the parts of Heimdall's stack that worked: Rust, axum, SQLite, Docker. Details in [technical-design.md](technical-design.md).

| Area | Direction | Note |
| --- | --- | --- |
| App form | PWA: manifest, service worker, home-screen install | iOS push needs the PWA added to the home screen (iOS 16.4+) |
| Login | Passkeys first, email magic link for new devices and recovery | Email provider not chosen; needs research. Until then, the admin sends a new device link to recover an account |
| Sessions | Server-side, opaque session id in the cookie | Heimdall's unsigned cookie is a known flaw; avoid it from day one |
| Notifications | Web Push (VAPID), per-device opt-in | Reminders, new todos, finance events |
| Frontend | Server-rendered HTML (Maud) + htmx, as in Heimdall | Decided; details in the technical design |
| Data | One-time import of Heimdall's expenses, payments, reminders and quotes | Members are matched by name; Discord ids are dropped |

Rollout: the new app runs alongside Heimdall until the import is verified. After that, Heimdall and the Discord bot are switched off.

## Open questions

- [ ] Which email provider sends login links? Needs research.
- [ ] Chat tool: confirm Telegram Topics or pick another.
- [x] Name: Asgard.
- [x] New features: none planned. First step is the switch to the app.
- [x] Quotes stay; the other fun extras are dropped.
- [x] Admin: Josef.
- [x] Expenses are always split equally across the whole crew.
