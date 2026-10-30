# Java runtime

The native runtime service detects Java from `JAVA_HOME`, `PATH`, standard Windows Java installation directories, and launcher-managed runtime directories. Detection runs off the UI thread and is cached for 30 seconds. A candidate is compatible only when its parsed major version matches the profile requirement and its reported architecture is x64.

## Managed Java 21 and 25

The launcher resolves Windows x64 Temurin JRE packages from Eclipse Adoptium’s versioned assets API for Java 21 and Java 25. It accepts only official API metadata, a GitHub release URL under `adoptium/temurin21-binaries` (for Java 21) or `adoptium/temurin25-binaries` (for Java 25), a valid SHA-256, and a bounded ZIP size. The streaming downloader verifies the published checksum and package size. Extraction rejects traversal and symbolic-link entries and enforces per-file and total expanded-size limits. The runtime is validated by running `java -version`, then atomically activated under the Tauri app-data `runtimes/java-<major>` directory. Metadata records the selected Java path and verified package digest. The runtime is not installed into the user’s global PATH.

Minecraft 26.1.2 profiles require Java 25; supported earlier profiles use Java 21. The launcher detects the profile’s required major and can install the matching managed runtime during install or repair. Other Java majors can be detected when installed locally, but managed installation currently supports only 21 and 25.

## Launch readiness

Java readiness is reported separately from Minecraft installation readiness. A profile still needs its version metadata, libraries, assets, loader and SNineClient files installed and verified before it can launch.
