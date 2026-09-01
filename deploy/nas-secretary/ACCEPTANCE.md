# NAS Secretary acceptance evidence

## 2026-09-01 Linux Remote pairing

Manual Remote pairing passed on 2026-09-01 at 1:50 PM MDT (UTC-06:00) using the official phone Remote interface and the NAS-hosted Codex App Server.

Evidence collected after ChatGPT reported `Connected`:

- The Secretary container remained `healthy`.
- `codex app-server daemon version` completed through the App Server control socket.
- The read-only `remoteControl/status/read` RPC returned `connected`, a non-empty `environmentId`, and a non-empty `installationId`.
- The read-only `remoteControl/client/list` RPC returned one paired mobile client with a non-null `lastSeenAt`.
- The persistent `state_5.sqlite` database contained one `remote_control_enrollments` row.
- That row has `remote_control_enabled` set to `NULL`. This deployment enables Remote Control explicitly with the App Server `--remote-control` argument, so startup does not depend on the stored preference column.
- No account ID, server ID, environment ID, installation ID, client ID, display name, or credential value was recorded in this file.

The [official Remote documentation](https://learn.chatgpt.com/docs/remote) currently documents connected Mac and Windows computers. This result verifies relay enrollment and a live mobile connection for image `sha256:b1a8e161de38fc8e3cf40f4b3b1ee9ce976739e154d03e211b93cfcd90dd9766` on the Synology NAS. It does not establish general OpenAI support for Linux Remote hosts.

## 2026-09-01 foundation thread and project-directory checks

An attached TUI created thread `01a05e95-9a2a-7883-a8f3-1a0f85fa7bd6`. Before its first turn:

- `thread/loaded/list` contained the thread.
- `thread/read` reported `idle`, `/workspace/secretary`, and an empty preview.
- `thread/list` returned no stored threads, the state database had no thread row, and no rollout file existed.
- The phone reported that no Remote threads were loaded from the host.

The first completed turn caused App Server to write the rollout, add the thread to `thread/list`, and populate its preview. This confirms that an empty TUI thread is loaded in memory but is not included in the stored thread list consumed by the phone.

The phone's new-chat flow attempted to create `/home/codex/Documents`. App Server's `initialize` response reports `codexHome` as `/home/codex/.codex`, while the Rust host contains no `Documents` default. Therefore the `Documents` selection is client-side; it matches the conventional directory under the parent of the reported `codexHome`. The client sends the filesystem operation to the host.

Compose now bind-mounts only `/home/codex/Documents` from `SECRETARY_DOCUMENTS_PATH`. Verification after recreating the Secretary service found:

- The container root filesystem remained read-only.
- `/home/codex/Documents`, `CODEX_HOME`, and `/workspace/secretary` were the only persistent writable mounts; `/tmp` remained the configured temporary `tmpfs`.
- The mounted directory had numeric ownership `1026:100` and mode `0750` on this NAS.
- `fs/createDirectory`, `fs/getMetadata`, and `fs/remove` succeeded through App Server for a temporary child under `/home/codex/Documents`.
- The App Server became healthy, Remote returned `connected`, one paired mobile client remained enrolled, and the attached TUI reconnected to the same stored thread after the service recreation.

## 2026-09-01 phone and restart acceptance

The phone displayed the stored TUI thread after its first completed turn. Jonathan opened that thread and sent `PHONE REMOTE CHECKPOINT.` from the phone at 2:38 PM MDT. App Server recorded the turn with `inputSource` set to `remoteControl`, completed it successfully, and saved both the user message and response in the same rollout. The attached TUI rendered both messages.

Only the long-lived Secretary service was then restarted. The separate TUI container stayed running and reconnected through the recreated Unix socket. After the service became healthy:

- `thread/loaded/list` contained the same thread.
- `thread/read` reported `idle` and retained the foundation-turn preview.
- `remoteControl/status/read` returned `connected`.
- `remoteControl/client/list` returned the same paired mobile client without generating another pairing code.

Jonathan then sent `PHONE REMOTE RECONNECTED` from the phone at 2:41 PM MDT. The live App Server notification reported `inputSource: "remoteControl"`, followed by a completed turn with status `completed`. The rollout saved the user message and exact assistant response `PHONE REMOTE RECONNECTED.`. The already-open TUI rendered the post-restart turn after reconnecting.

This completes the foundation acceptance test for phone thread visibility, a phone-originated turn in the TUI thread, persisted enrollment across an App Server restart, TUI reconnection, and a second phone-originated turn after restart.
