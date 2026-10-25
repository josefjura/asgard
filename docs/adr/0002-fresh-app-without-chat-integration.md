# A fresh app with no chat integration

The Crew left Discord, and Heimdall was built around it: login, identity (`discord_id`), notifications and todos all depended on Discord, and its bot mostly served finance entry and jokes. We built Asgard from scratch instead of adapting Heimdall, and made it independent of any chat tool: no bot, no messaging, and notifications go out as Web Push from the app itself. The choice of chat tool (Telegram Topics is the front-runner) therefore never blocks the product. Heimdall remains a reference for the old finance logic and the source of a one-time data import.

## Consequences

- An optional one-way webhook into a chat channel may come later; nothing may depend on it.
- Heimdall and its bot are switched off once the import is verified.
