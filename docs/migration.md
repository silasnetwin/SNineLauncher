# Migration

The legacy launcher remains at C:\Users\silas\Documents\SNineLauncher - Kopie and is read-only during this rebuild. The requested unsuffixed path was absent at audit time. The new app ID (de.snine.launcher.next) and application data directory are separate from the legacy installation.

No legacy data is imported automatically. A future importer must inventory recognized files, show a preview, validate paths and formats, copy data into a staging directory, verify hashes/metadata, and only then expose imported profiles. It must never move, delete, or overwrite the source. Secrets are not migrated from plaintext locations; account sign-in should be repeated into the OS credential store. Until these rules are implemented, migration is not available.
