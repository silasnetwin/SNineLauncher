# Testing and validation

## Commands

    npm install
    npm test
    npm run lint
    npm run build
    cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
    cargo test --manifest-path src-tauri/Cargo.toml
    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
    npm run tauri:build

The frontend test script uses Vitest; focused behavior tests should be added alongside features. Rust unit tests cover the seed schema, selected profile, name validation, and missing profile selection. The 1280×760 Vite UI smoke check exercised navigation to Cosmetics, back to Library, profile creation/details, and the Downloads panel. The native Windows build requires MSVC tooling and WebView2. Do not treat a Vite build as a successful Tauri release build.

## Gaps

No automated behavior tests have been added yet. Full UI, launch lifecycle, account, offline and download testing requires implementations and test fixtures. Performance budgets are defined in performance-budget.md; release process memory and CPU have not been measured.
