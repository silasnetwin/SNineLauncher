# Legacy feature matrix

Audited read-only from C:\Users\silas\Documents\SNineLauncher - Kopie on 2026-10-15. That folder is the only launcher checkout present; the requested unsuffixed C:\Users\silas\Documents\SNineLauncher path does not exist. The legacy app reports phase 1–10 functionality but explicitly says it is not a releasable release: remote updater disabled, production endpoints forbidden, cloud linking disabled without an approved SNine backend, and updater signing material unavailable. This is a historical audit of the rebuilt launcher as it stood on 2026-10-15, not an authoritative description of the current tree. Since then, authentication, Minecraft installation/launch, runtime management, Modrinth content management, and profile backup/restore have been implemented further. Use the current README and feature-specific documents for present capabilities; re-audit this table before using it for release decisions.

| Feature | Legacy implementation | Backend/API dependency | New implementation | Status |
|---|---|---|---|---|
| Desktop shell and window | Tauri 2 custom non-resizable title bar, React 19/Vite, compact shell; stable ID site.s9lab.launcher | Tauri window APIs | Resizable Tauri 2 shell, custom titlebar, independent ID | Partial: shell implemented |
| Startup/local state | Rust foundation, app paths, SQLite, operation recovery, settings bootstrap | Local SQLite and OS app-data paths | SQLite WAL, local bootstrap, split app-data folders | Partial: schema v1/bootstrap only |
| Microsoft authentication | Device-code OAuth, Xbox/XSTS and Minecraft token exchange, ownership checks, session refresh/cancellation | Microsoft identity and Xbox/Minecraft services | None | Deferred; must use OS credential store |
| Account/session handling | Multiple accounts, active account selection, relogin state, offline eligibility policy | Microsoft/Minecraft auth | Sign-in entry point placeholder | Deferred |
| Profiles | Isolated profiles, revisions, duplicate/archive/trash/restore/favorites, profile/account assignment | Local filesystem and SQLite | Local profiles with selected profile persistence | Partial |
| SNineClient versions | Dedicated SNine client bridge and update check/download | SNine client delivery endpoint; legacy security docs prohibit production endpoint | Profile model has client/Minecraft versions | Deferred; no endpoint assumed |
| Minecraft versions/loaders | Vanilla/Fabric catalog, NeoForge capability status, runtime intent and component catalog | Mojang metadata/assets/libraries, Fabric metadata; NeoForge and SNine component may be unconfigured | None | Deferred |
| Java/runtime | Java major policy, managed/system runtime, detection and secured runtime install | Local Java; vendor/runtime artifacts | None | Deferred |
| Install/repair/launch | Runtime install/repair, launch lifecycle, process status/stop, logs, instance settings and quick play | Mojang metadata/assets/libraries, Microsoft auth, local process/filesystem | Install CTA only; does not execute | UI placeholder |
| Downloads | Rust download service, limits/policy, content and runtime progress | Mojang/Fabric/Modrinth/SNine artifact sources | Downloads view placeholder | Deferred |
| Mods and content | Modrinth search/detail/tags, resolver, dependencies/conflicts, install/update/toggle/remove, batch install, safe local files | Modrinth API/CDN; local archives | None | Deferred |
| Modpacks and datapacks | MRPACK/profile import/export, safe archive extraction, world datapacks, lockfile projection | Modrinth plus local filesystem | None | Deferred |
| Recommendations | defaultModRecommendations and content discovery surfaces | Modrinth metadata | None | Deferred |
| Backups, restore, rollback | Restore points, content update preview/apply, profile revisions and rollback | Local filesystem/SQLite | None | Deferred |
| Launcher update | Tauri updater configured in legacy config but disabled by security policy; signing key unavailable | GitHub release endpoint and signing key | No updater configured | Intentionally disabled |
| Client/content updates | Separate update channels, policies and previews | SNine delivery and Modrinth | None | Deferred |
| Cloud sync | Local two-device sync/conflict model exists; linking shown disabled | Approved SNine backend not configured | None | Deferred; no live service contract |
| Cosmetics/player | 3D player and skin/cape/wings/halo local preview; custom and vanilla cape commands | Microsoft profile and cosmetic service for account data; local asset cache | Cosmetics placeholder | Deferred |
| Settings/theme/i18n | System/light/dark/high-contrast, language, density, nav, backgrounds, reduced motion, download policy; typed translations | Local configuration | Default dark visual system; settings placeholder | Partial |
| Shell/process commands | Native file/folder opening, process launch/stop and window operations; restricted command surfaces | OS shell/process | Window controls only | Partial |
| Logs/errors/diagnostics | Structured app paths/logging, operation recovery, latest game log tail, error codes | Local files | Local app directories created; no logger UI | Partial |
| IPC contracts | contracts/ipc-contracts.json v14 and generated TypeScript, broad phase1–10 command surface | Tauri invoke/events | Centralized adapter for bootstrap/profile commands | Partial; current Rust/TS contracts small |
| Security/paths/archive | Path registry/validation, safe archive handling, fail-closed auth and offline policies, content hash verification | OS credential store; provider metadata | Minimal Tauri capabilities/CSP; SQLite profile inputs validated | Foundation only |
| Installer/Windows release | Phase 10 cleanroom, NSIS lifecycle and source/package verification; unsigned release explicitly not distributable | Windows MSVC, local signing unavailable | Tauri NSIS/MSI config, distinct app ID | Not built/validated yet |

## Audit sources

Inspected legacy package.json, README.md, Tauri config, Rust auth, profiles, minecraft, runtime, modrinth, content, updates, cloud_sync, capes, storage, security, and app modules; generated IPC contract; UI pages/components; phase architecture/security/external-blocker docs; and test inventory. Relevant sources include PHASE-3-ARCHITECTURE.md through PHASE-10 docs, PHASE-8-EXTERNAL-BLOCKERS.md, PHASE-10-SECURITY.md, FINAL-COMPLETION-REPORT.md, contracts/ipc-contracts.json, and src-tauri/tauri.conf.json.

## Exclusions and decisions

- Do not carry forward the legacy updater endpoint or embedded public key: legacy security documentation describes signing material as compromised/unavailable and updater as disabled.
- Do not enable cloud sync until a service contract and approved backend exist; local conflict architecture alone is not a live sync service.
- Legacy theme variants are preserved as a future preference capability, but the rebuilt product starts with one dark theme as requested.
- Keep profile/runtime/content and backup features in scope for staged implementation; none are declared obsolete solely because their UI is not in this first foundation.
