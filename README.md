# SNine Launcher

A Windows-first desktop launcher built with Tauri 2, Rust, React, TypeScript and Vite. Native services own Minecraft installation, profile files, Java runtimes, authentication, downloads and game process management. SQLite stores local launcher state; account credentials are kept through the Windows credential store.

## Current features

- Minecraft version and Fabric/Quilt loader discovery, profile creation and management.
- Managed Minecraft installation, verification/repair, launch lifecycle and game logs.
- Java detection plus verified managed Temurin Java 21 and 25 installations.
- Microsoft device-code sign-in, local account/session handling, and owned Minecraft skin/cape selection.
- Modrinth mod and resource-pack discovery/installation, local profile content, and profile backup/restore.
- Local SQLite settings, favorites, profile groups, memory/window options, download progress and storage reporting.

## Development

Requirements: Windows 10/11 x64, Node.js 20+, npm, Rust stable with the MSVC toolchain, Microsoft C++ Build Tools and WebView2.

```powershell
npm install
npm run dev
npm run build
npm run tauri:dev
npm run tauri:build
```

`npm run lint` runs strict TypeScript checking; `npm test` runs frontend tests. The browser preview uses demo data; native functionality requires Tauri.

## Project layout

- `src/`: React interface, typed IPC adapter, styles and UI components.
- `src-tauri/src/`: Rust commands, SQLite persistence, download/runtime/install/auth/content services.
- `docs/`: architecture, security, provider boundaries and feature notes.

## Release boundaries

The launcher is under active development. SNineClient delivery still depends on the configured artifact source and does not yet have a signed immutable publisher manifest. SNine-specific custom cosmetics and cloud sync require approved service contracts. Signed launcher updates are configured; publishing requires the private signing key and the reviewed GitHub release workflow described in [updater.md](docs/updater.md). Users on an older launcher build need one manual installer update before in-app updates can take over. Review [security.md](docs/security.md), [installation.md](docs/installation.md), [profile-backup.md](docs/profile-backup.md), and [cosmetics.md](docs/cosmetics.md) before distributing a build.
