# Custom App Server API behavior

## Persisted item timestamps

Each `thread/items/list` entry includes the containing `turnId`, the item's
persisted `createdAtMs` Unix timestamp in milliseconds, and its full `item`.
Older servers may omit `createdAtMs`. Omit `turnId` or pass `null` to page items
across the thread. Item cursors can be reused with or without `turnId`.

## Remote Control turn origin

`turn/started` includes optional `inputSource`: `remoteControl` for turns started
through Remote Control and `appServerClient` for other App Server clients.
Older servers omit the field. The TUI uses it to suppress desktop notifications
for Remote Control turns while preserving Remote Control synchronization.

## Current thread metadata in listings

With the state database available, `thread/list` returns each thread once using
its current metadata and selected rollout, including after a working-directory
change or revert. The default scan still discovers unindexed legacy rollouts.

## Persisted active-turn input

With `capabilities.experimentalApi = true`, `thread/queue/add` accepts `steer: true`
to offer a persisted message to the active regular turn at its next model request.
This wakes `collaboration.wait_agent`; running commands and approval dialogs keep
running. Optional `additionalContext` is recorded with the input, including the
TUI's original prompt timestamp. The TUI reloads the queue on resume and after
`thread/queue/changed`.

`thread/queue/take` accepts `threadId` and `queuedSubmissionId`, removes the entry
under the same dispatch lock used by model delivery, and returns
`queuedSubmission`. The value is `null` if another consumer already removed it.
The TUI uses this operation for Up-to-edit and prepends the returned text to the
current draft. It leaves the editor unchanged if the message was consumed first.
