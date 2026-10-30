# Downloads

## Implemented now

`src-tauri/src/downloads.rs` owns a reusable Reqwest client, limits independent transfers to four concurrent jobs, streams response chunks to disk, writes to a `.part` file, retries failed transfers up to three attempts with short exponential delay, resumes when the server answers the requested byte range correctly, optionally verifies SHA-256, syncs the file, and renames the completed partial file. Progress is emitted on the Tauri `download-progress` event at roughly 5.5 Hz while data is flowing. Cancellation uses Tokio cancellation tokens.

## Current limitations

The queue and cancellation tokens live in memory. No pause, retry command, queue snapshot, restart recovery, persistent job history, install association, allowed-host registry, download byte cap, ETag/If-Range validation, or disk-space preflight is implemented. A partial file may be resumed using the same destination name, but the service does not persist an origin/validator record across app restarts. These gaps must be closed before this service handles production install artifacts. UI progress is event driven; there is no polling loop.
