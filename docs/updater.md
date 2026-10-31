# Launcher updates

The desktop launcher checks the configured GitHub release feed six seconds after startup. It offers a signed update and restarts only after the user chooses **Update and restart**. Tauri verifies the package signature against the public key embedded in `src-tauri/tauri.conf.json` before installation.

SNineClient artifacts are published together under `client-v<version>`, with a manifest entry for each supported Minecraft port. The launcher first reads `https://snc.silasnet.win/updates/manifest.json`, selects the artifact matching the profile's Minecraft version, and verifies its size and SHA-256 before replacing the installed client. GitHub's matching client release is the fallback if the home update host is unavailable. Current managed ports are 1.21.11, 26.1.2, and 26.3. Add a completed port to the publishing task before advertising it as launcher-supported; 1.8.9 is currently excluded.

## Signing key setup

The matching private key is stored outside this project at:

`C:\Users\silas\Documents\snine-launcher-next-updater`

Keep that file private, back it up securely, and never commit or send it in chat. Add its contents as the `TAURI_SIGNING_PRIVATE_KEY` Actions secret in the `silasnetwin/SNineLauncher` GitHub repository. The corresponding public key is embedded in the Tauri config; do not replace it without creating a coordinated key-rotation release.

## Publishing

Push a tag such as `launcher-v1.0.15`. GitHub Actions builds signed NSIS and MSI installers and generates `latest.json`. The workflow leaves the release as a draft. Review the assets and updater manifest, then publish the release so the launcher's `releases/latest/download/latest.json` endpoint serves it.

The first install of this app identity still needs a normal installer. Existing launcher builds without this updater configuration cannot update themselves to this build; give those users the new installer once. Subsequent releases can update in-app.
