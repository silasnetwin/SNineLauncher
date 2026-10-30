# Profile and mod management status

Profile metadata is stored in SQLite and migrated additively. Profiles have a stable
ID, name, favorite/hidden state, Fabric loader identity, SNineClient flag, icon
identifier, and optional memory limit. Rename, duplicate, and safe deletion operate
on the profile ID; duplication creates an uninstalled profile and does not copy a
game directory.

The current install/launch provider remains the confirmed SNineClient + Fabric
pipeline. Profile creation therefore creates a profile for that supported path. The
loader enum includes future values for schema evolution, but Vanilla, Forge,
NeoForge, Quilt, arbitrary Minecraft versions, and Fabric-only profiles without
SNineClient are not installable through this provider yet and are not presented as
working choices in the UI.

Installed Fabric profiles can search Modrinth for compatible projects by Minecraft
version, resolve required dependencies, stream files through the existing bounded
download service, verify Modrinth SHA-512/SHA-1, stage the complete plan, and publish
with rollback support. Declared Modrinth incompatibilities against managed projects
are rejected before download. User-provided local Fabric JARs are listed and can be
enabled, disabled, or removed. Managed mods can be checked for newer compatible
releases and updated individually or together through the same staged installer.

Known limitations: dependency version constraints and optional dependencies are not
resolved; conflicts with untracked local JARs cannot be identified; Modrinth release
selection uses the latest compatible release rather than a user-selected version.
Resource packs, shader packs, profile export/import, migration between Minecraft or
loader versions, and a version-specific block icon picker are not implemented. A
profile icon identifier can be persisted, but no resource-backed picker is exposed.

Mod search uses a reusable Rust HTTP client, bounded response sizes/timeouts, a
five-minute in-memory cache, compatible Fabric/Minecraft filtering, and a 180 ms
frontend debounce with stale-result suppression. SilasCraft was inspected read-only;
see [silascraft-reference.md](silascraft-reference.md) for the engineering patterns
used as reference.
