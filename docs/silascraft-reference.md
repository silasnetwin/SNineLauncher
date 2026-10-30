# SilasCraft engineering reference

Inspected `C:\Users\silas\Documents\dev\silascraft` read-only. No files in that project were changed.

Useful patterns observed:

- Marketplace HTTP access is centralized behind a small client with explicit upstream-host allow-lists, bounded response sizes, a reusable HTTP client, and timeouts.
- Metadata responses are cached in memory with expiry and bounded entry/byte counts; cache hits return copied data rather than exposing mutable cache storage.
- UI fetches use `AbortController` during page changes/unmount so stale searches do not overwrite newer results.
- Errors are normalized at the service boundary, and UI updates retain the last useful state while requests are pending or canceled.

SNine can apply these patterns to its own Modrinth client: allow-listed HTTPS hosts, response limits, a bounded metadata cache, cancellable searches, and stale-response guards. SilasCraft-specific auth, data models, and business logic are not reused.
