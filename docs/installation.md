# Installation status

The native installer downloads the 1.21.11 client from the configured legacy artifact URL and bundles verified builds for Minecraft 26.1.2 and 26.3. Every JAR is validated against its Fabric metadata and the selected profile's exact Minecraft version before activation in `instance/mods`. Bundled 26.x builds use Java 25; 1.21.11 uses Java 21. The computed SHA-256 is local integrity metadata, not publisher authentication. The bundled builds do not show remote updates until a trusted per-version release manifest is available.

Managed Java 21 can be installed separately through the official Eclipse Temurin metadata and verified ZIP flow documented in [runtime.md](runtime.md).

The current client JAR install is not a complete playable Minecraft installation. Mojang's version manifest, client JAR, asset index and objects, libraries/natives, Fabric loader profile and classpath/launch arguments are not yet installed. The database's `clientReady` flag means only that the SNineClient JAR passed its checks; it must not be treated as overall Minecraft readiness. The app keeps these states visibly distinct and does not claim PLAY works.

Before a distributable release, SNine needs a versioned, trusted client artifact manifest with immutable version, compatibility, file size and publisher checksum/signature information. The legacy tree did not provide a production manifest contract suitable for secure updates.
