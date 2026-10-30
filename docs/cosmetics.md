# Minecraft account cosmetics

The Cosmetics tab reads the authenticated account's Minecraft profile through the Minecraft Services profile endpoint. The bearer token stays in the Rust authentication service and is never sent to the React frontend. Only HTTPS texture URLs hosted by `textures.minecraft.net` are returned for display. Account and friend avatars resolve the UUID through Mojang's session profile endpoint, decode its `textures` property in Rust, validate the official texture host, and crop the front face plus hat overlay from the real skin texture in the UI. Profiles without a Mojang skin show the initials fallback.

The page lists account-owned skins and capes, identifies active selections, and lets the user activate an owned skin, equip an owned cape, or hide the active cape. The native service checks each requested cosmetic against the account's current owned list before calling Minecraft Services. Selecting a cosmetic changes the player's Minecraft account and requires a direct click in the UI.

The page includes an interactive Three.js-powered Minecraft player preview with the skin model and optional cape. Drag to rotate the player; owned skins and capes can be selected for preview without applying them to the account. SNine-specific wings, halos, and custom cosmetics are not implemented here.

Users can upload a PNG skin and select the Classic (Steve) or Slim (Alex) model. Before upload, the native service rejects symlinks, non-PNG content, files larger than 1 MiB, and images other than Minecraft's 64×64 or legacy 64×32 dimensions. The launcher asks for confirmation before the account skin is changed; it refreshes the owned cosmetics after success.

The account control in the title bar supports multiple Microsoft/Minecraft identities. Refresh tokens remain in the Windows Credential Manager under account-specific targets, with an account-keyed DPAPI vault as the secure fallback. The database stores only account identity metadata and the active selection. Switching refreshes the chosen account's Minecraft session; signing out removes only that account and returns to another saved account when available.
