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
