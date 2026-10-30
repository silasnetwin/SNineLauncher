# Security status

## Current protections

- SQLite work is moved to `spawn_blocking` from async Tauri commands.
- Profile names are trimmed and length-limited; selection and flag updates require an existing ID.
- Favorite/hidden SQL column identifiers are selected from a closed Rust enum, not caller input.
- Download URLs are parsed and limited to HTTP(S); destination filenames must be a single normal path component; optional SHA-256 must be 64 hexadecimal characters.
- Downloads stream to a partial file, verify an optional hash, sync, and rename only after success.
- Secrets are not present in the new profile schema.

## Gaps before production installation

The generic download API does not restrict hostnames or enforce HTTPS-only; it must only be callable by trusted service code for production assets. There is no path registry, installation root validation, safe archive extraction, download manifest signature validation, credential vault, process executable allow-list, installer rollback/recovery, or structured redacted error model. Never expose arbitrary path deletion or arbitrary process launch through frontend-provided strings.
