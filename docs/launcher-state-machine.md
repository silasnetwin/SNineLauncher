# Launcher state model

The current profile model stores stable identity and local library flags. Installation and launch are not implemented, so the frontend must not imply that a profile is playable based only on `installed`.

## Current states

- Profile: `selected` is projected from the selected-profile setting; `favorite` and `hidden` are persisted booleans; `installed` is a legacy placeholder field and is not currently set by a real installer.
- Download: `queued`, `downloading`, `completed`, `cancelled`, `failed` are emitted by the Rust transfer service. Jobs are not persisted and cannot be queried after restart.
- Activity: profile created, opened, favorited/unfavorited and hidden/restored events are persisted (bounded to 100); install/launch events are not emitted because those operations are not wired.
- Authentication, installation and launch: no state machine is currently wired.

## Intended operation transitions

```text
install: resolve → queued → downloading → verifying → installing → installed
                                          ↘ failed / cancelled
launch:  ready → starting → running → exited
                           ↘ failed
update:  current → update available → staging → verifying → committed
                                           ↘ rollback
```

Each long operation should be keyed by a native-created operation ID and guarded against conflicting operations on the same managed profile. The UI should consume state snapshots plus coalesced events; it should not own authoritative transitions.
