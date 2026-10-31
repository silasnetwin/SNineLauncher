# S9 Client pack catalog

`catalog.json` is the curated pack list shown in the launcher's **Mods → S9 packs** tab. The launcher fetches the public file from this repository on startup and falls back to the same bundled copy when GitHub is offline.

## Add or update a pack

Each pack has a unique lowercase `id`, a short name and description, one loader, an explicit list of supported Minecraft versions, and Modrinth project IDs/slugs. The launcher only displays entries matching the installed profile's loader and Minecraft version. Mod files are selected from Modrinth for that exact version and Fabric; required dependencies are installed by the existing resolver.

Keep the pack list focused and check each Modrinth project's game-version and Fabric support before adding it. A pack update appears for users once `catalog.json` is published to the `main` branch of `silasnetwin/SNineLauncher`. Existing builds retain their bundled list if GitHub cannot be reached.

No NoRisk code, pack definitions, branding, or repository names are used here; this is the SNine catalog format.
