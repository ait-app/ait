# ADR-067: Account discovery and on-demand reverse relay

Status: implemented for the desktop client, 2026-10-01.

AIT nodes register automatically after account login. A node backed by the local Rust
runtime opens one outbound control WSS. Discovery polls the center independently of
HostRuntime; only explicitly selected hosts become remote work connections.

The desktop main process owns the default account API base,
`https://dash.ait-app.com:8443/api`. An omitted or blank login center resolves to this
base, and the account snapshot supplies it to the renderer. Login, discovery,
control and data URLs all preserve the `/api` prefix and port; WSS is derived from
the same HTTPS base. No credentials are bundled. The Host settings page and add-Host
dialog expose email/password login, with an optional service override under
service settings. Account IPC accepts `email` and `password`; the main process trims
and lowercases the email before sending `{ email, password }` to `/v1/auth/login`,
while preserving the password exactly. Restored accounts keep their saved service address rather than
being migrated to the default. Valid saved credentials automatically restore the
node activation when OS secret storage is available.

The desktop main process owns the user JWT and renews node authorization. The new
`server-relay` crate receives one-use control grants through authenticated local API
routes. It knows only the actual local runtime address/token, and creates an independent
reverse data WSS per access. It never accepts an arbitrary local destination from the
center. Control loss cancels visits targeting that control epoch; logout and node lease
expiry cancel all incoming and outgoing visits for that node.

The `connection.single.v1` required capability selects a single physical business WS.
The capability offer remains bounded (256 unique entries). Four internal capability
workers preserve operation order within metadata/browser/schedule, terminal/voice,
filesystem and provider groups. Each worker retains the existing 16-subscription
budget, so a logical single connection has at most 64 subscriptions. This grouping
keeps a slow Git operation off the terminal and ping path. A release is acknowledged
after all workers have processed it; observers remain owned by the physical connection.
Inbound workers have two queue slots each; outbound byte and message budgets remain
shared, including the active write. The physical writer rotates across the four
lanes. Admission rejects requests before execution, and event/binary overflow closes
the connection rather than dropping user input. No network reconnect replays writes.

The center is trusted, uses HTTPS/WSS, and authorizes both sides by same-account node
sessions. This supersedes ADR-061's removal of the older public-key relay protocol;
the historical relay URL and pairing-key format are not reused.

Independent download sessions stream from the fixed local token-based HTTP endpoint
to a desktop-owned temporary file. The main process owns user and relay credentials;
only trusted application windows can invoke account IPC. Navigation, window closure,
Host switching and logout cancel their transfers. Saved JWTs require OS secret storage.

Design and API owner: `ait-server/docs/architecture.md` in the sibling repository.

Test coverage: not measured during local implementation. Focused transport, protocol,
relay and two-real-runtime terminal/download tests passed; workspace coverage is deferred until
commit preparation.
