# Microsoft and Minecraft authentication

## Legacy contract (read-only discovery)

The legacy launcher at `C:\Users\silas\Documents\SNineLauncher - Kopie` implements Microsoft's consumer **device-code** flow. Its public desktop application ID is `e686aebd-d575-4472-b163-b0c54f388f43`; scopes are `XboxLive.signin offline_access`. It requests a code from `https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode`, polls `https://login.microsoftonline.com/consumers/oauth2/v2.0/token` at the server-provided interval (minimum five seconds), and exchanges the Microsoft token through Xbox user authentication, XSTS, Minecraft Services login, Minecraft Store entitlement check, and Minecraft profile lookup.

The service sequence is:

1. `https://user.auth.xboxlive.com/user/authenticate` (`RPS`, `d=<Microsoft access token>`, relying party `http://auth.xboxlive.com`).
2. `https://xsts.auth.xboxlive.com/xsts/authorize` (`RETAIL`, relying party `rp://api.minecraftservices.com/`).
3. `https://api.minecraftservices.com/authentication/login_with_xbox` with `XBL3.0 x=<uhs>;<XSTS token>`.
4. `https://api.minecraftservices.com/entitlements/mcstore`; Java access is accepted only when `game_minecraft` or `product_minecraft` is present.
5. `https://api.minecraftservices.com/minecraft/profile` for the actual UUID and username.

The legacy app stores credentials with the Windows native credential vault (`keyring`) and keeps only account identity in SQLite. It maps Xbox XErr 2148916233 to missing Xbox profile, 2148916235 to region availability, 2148916236/2148916237 to age verification, and 2148916238 to family approval. The launcher has a separate SNine backend session exchange; that is not part of Microsoft/Minecraft sign-in and is not performed by this auth service.

## Rebuilt launcher implementation

Rust owns the device-code flow and all token exchanges. The UI receives only the device code/verification URL during authorization and safe account metadata (`uuid`, `username`, authentication timestamp) after sign-in. Durable secret state is reduced to the Microsoft refresh token, stored under the stable Windows generic credential target `SNineLauncher/MicrosoftAuth/<minecraft-uuid>` (service `SNineLauncher/MicrosoftAuth`, generic credential type). The short-lived Microsoft access, Xbox, XSTS and Minecraft tokens are rebuilt after restart and held only in process memory. The older full-session `SNine Launcher` / `minecraft:<uuid>` entry is migrated by extracting only its refresh token and then removed.

The Credential Manager adapter uses raw secret bytes (rather than the keyring password API's UTF-16 conversion) and records only target/account identifiers, operation, payload/blob lengths, keyring error type, and numeric Windows error code in local diagnostics. It never records secret contents. If Credential Manager rejects a write, a CurrentUser DPAPI encrypted `credentials.dat` in the launcher's app-data `auth` directory is used; replacement uses `MoveFileExW` with replace-existing and write-through flags. The app-data directory inherits the current Windows user's profile ACL. SQLite stores safe identity metadata only. No token is placed in browser storage, IPC event data, or logs.

Startup loads safe SQLite account metadata immediately; then `auth_restore` reads the secure refresh credential and rebuilds the complete Microsoft → Xbox → XSTS → Minecraft chain asynchronously. In-process Minecraft sessions use a five-minute expiry safety margin. A credential-save failure keeps the verified account/session in memory and exposes a retryable persistence warning instead of discarding the live login. Logout cancels pending device authorization, deletes the stable Credential Manager entry, legacy entry and DPAPI fallback, clears in-memory secrets and safe metadata, then emits a safe logged-out state. The launch-facing Rust method `require_valid_minecraft_session` refreshes if needed; its IPC wrapper returns only safe account metadata and zeroizes/discards the token at the frontend boundary.

The device flow validates the verification URI as HTTPS on a Microsoft-owned host, honors provider poll intervals and `slow_down`, supports cancellation, and does not include authorization codes or tokens in user-facing errors. Unit tests use local fixtures/helpers and do not contact Microsoft services.

## Verification status

The previous full-session credential write failed with a generic message because the old implementation discarded the keyring error. The new adapter records safe diagnostics and no longer persists the large session object; the exact old Windows error cannot be recovered retroactively. Current end-to-end save, restart/refresh and logout still require an owner-controlled sign-in on this machine. No account credentials should be entered into Codex or logs. The account menu still uses a username-derived initials fallback. The Cosmetics tab separately fetches owned Minecraft skins and capes after authentication; bearer tokens remain in the native Rust service.
