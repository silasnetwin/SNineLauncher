# Launcher architecture

The new application is React + TypeScript over Tauri 2, with Rust owning local persistence, network transfers, and future native process/filesystem work. The frontend command surface is centralized in `src/lib/ipc.ts`; Tauri commands live in `src-tauri/src/commands/mod.rs`; SQLite access is in `src-tauri/src/database.rs`; generic downloads are in `src-tauri/src/downloads.rs`.

```text
React presentation
  → typed src/lib/ipc.ts
  → Tauri command
  → Rust database / service
  → local SQLite, app-data filesystem, HTTP, later native process
```

SQLite is the source of truth for desktop profiles, selected profile, favorite/hidden flags, recent selections and a bounded activity log. Frontend state is an optimistic view cache. The browser-only Vite preview uses a demo profile and localStorage only for its recent-menu preview. The canonical app icon and shell presentation are deliberately outside this functional port.

## Current boundaries

- `bootstrap` returns profile snapshots, selected profile, recent profile IDs and schema version.
- Profile creation and selection are native SQLite operations.
- Favorite and hidden mutations are stored on the profile row; the frontend rolls optimistic changes back on IPC errors.
- Profile creation/selection and favorite/hidden changes append meaningful activity entries. The list is capped at 100; transfer progress is not logged.
- Download jobs are currently owned by an in-memory Rust service and emit Tauri events. Their queue does not survive process restart.
- Install, runtime, launch, authentication, SNine API and persistent activity services are not wired.

See [legacy functional port](legacy-functional-port.md) for the read-only capability audit.
