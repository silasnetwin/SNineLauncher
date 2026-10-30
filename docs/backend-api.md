# Backend API and provider contracts

## Current new launcher

There is no SNine backend client or production manifest API in the new source tree. The only outbound network service is the generic Rust HTTP downloader, which accepts an HTTP(S) URL and a single filename from the caller. That generic interface is not a trusted version manifest and must not be used by an install button with arbitrary URLs.

## Legacy contracts observed read-only

- SNine API origin in legacy `src-tauri/resources/backend-endpoints.json`: `https://api.s9lab.site/api/v1`.
- SNine websocket origin in the same file: `wss://ws.s9lab.site`.
- Legacy source embeds a backend certificate and uses a session exchange after Minecraft authentication. The certificate and authentication behavior must be reviewed against current server operations before any new client is implemented.
- Microsoft OAuth/Xbox/Minecraft Services flows use their standard HTTPS endpoints in legacy `auth/microsoft.rs`.
- Mojang version metadata, Fabric metadata/Maven, NeoForged Maven, and Modrinth API/CDN are external provider contracts, not SNine backend APIs.

Do not infer a supported client release manifest from the legacy hardcoded SNine client JAR URL. New installs need an explicit signed or otherwise trusted manifest containing version, target path, byte size, SHA-256, runtime requirements, and content dependencies. Production origins, trust anchors, authentication scopes, timeout/retry policy and schema ownership must be confirmed before enabling remote install/update.
