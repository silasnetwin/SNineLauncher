# Profile backup and restore

From a profile's Overview, create a ZIP backup or restore a previously created backup. Archives include the complete profile `instance` tree, including saves, mods, resource packs, options and profile install metadata. Shared Minecraft libraries/assets and managed Java runtimes are not copied.

Backups are written to a user-selected location. A running Minecraft profile cannot be backed up or restored. Restore requires a valid SNine backup archive for the same profile ID, checks entry paths and archive size, rejects symbolic links, stages extracted files, checks available disk space and swaps the profile instance into place. Restore replaces the current instance files; confirm the operation in the launcher before selecting the archive.

The launcher imposes a 50 GiB expanded-size and 250,000-file limit. Keep backup files somewhere outside the launcher-managed profile directory.
