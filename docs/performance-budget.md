# Performance measurements and budget

## Instrumentation

- Rust debug startup logs `[startup] native core ready` after app-data directories, SQLite, and service initialization.
- Frontend `src/lib/startup.ts` records shell mount, first animation-frame paint, and local cache completion in development.
- Rust debug Play instrumentation records `PLAY_CLICK`, `AUTH_READY`, `INSTALL_STATE_READY`, `RUNTIME_READY`, `LAUNCH_CONFIG_READY`, and `JAVA_SPAWNED`. Timings are local only and never transmitted. `PLAY_CLICK` starts when the native command begins, so it excludes the frontend-to-native IPC delay.

## Windows development observations (2026-10-15)

- Native core ready: 433–470 ms across several starts. This excludes WebView first paint and is not a full startup benchmark.
- Full cold Minecraft setup completed from official Mojang/Fabric metadata for Minecraft 1.21.11. The asset index contained 4,591 objects (452,678,632 bytes declared); previously valid shared objects were reused. A later warm validation/setup pass took 8.5–12.4 seconds and downloaded 0 bytes.
- Warm Play path: native command start to Java process spawn was 24.1 ms. Auth was already restored and cached; install metadata and Java 21 were present. This is one development run, not a broad performance claim.
- Game log showed Fabric Loader 0.19.5 and `snineclient 1.1.5`; the Minecraft resource manager reported a reload about 10 seconds after the observed Java start. The game window title was `SNine Client 1.1.5 | Minecraft 1.21.11` and the process was responsive.
- The frontend shell/first-frame timestamps were not captured from WebView devtools in this run. No cold-start or multi-machine benchmark has been performed.

## Current behavior

- Local SQLite bootstrap runs asynchronously; no network request blocks shell render.
- Search uses React `useDeferredValue`, and profile cards are memoized.
- Download transfer streams to disk, uses bounded concurrency, resume/retry/checksum behavior, and coalesces Minecraft asset events into one aggregate progress task.
- The shared download service caps concurrent transfers at four; asset scheduling is separately capped at eight tasks and is further constrained by the shared transfer slots.
- Minecraft asset verification streams hashes through a fixed 64 KiB buffer rather than buffering whole objects in memory.

## Measurement targets

Track native core ready, shell mounted, first useful frame, SQLite bootstrap completion, install click to first byte, and Play click to Java spawn. Record cold and warm runs with machine details before setting user-facing latency claims. Download progress should rerender only its relevant UI state; no idle polling should be introduced.

