# Launcher updates

The desktop launcher checks the configured GitHub release feed six seconds after startup. It offers a signed update and restarts only after the user chooses **Update and restart**. Tauri verifies the package signature against the public key embedded in `src-tauri/tauri.conf.json` before installation.

## Signing key setup

The matching private key is stored outside this project at:

`C:\Users\silas\Documents\snine-launcher-next-updater`

Keep that file private, back it up securely, and never commit or send it in chat. Add its contents as the `TAURI_SIGNING_PRIVATE_KEY` Actions secret in the `silasnetwin/SNineLauncher` GitHub repository. The corresponding public key is embedded in the Tauri config; do not replace it without creating a coordinated key-rotation release.

## Publishing

Push a tag such as `launcher-v1.0.13`. GitHub Actions builds signed NSIS and MSI installers and generates `latest.json`. The workflow leaves the release as a draft. Review the assets and updater manifest, then publish the release so the launcher's `releases/latest/download/latest.json` endpoint serves it.

The first install of this app identity still needs a normal installer. Existing launcher builds without this updater configuration cannot update themselves to this build; give those users the new installer once. Subsequent releases can update in-app.
