# Personas Codex runtime

This directory contains the reproducible Codex image used by the NAS personas
service. The image is built from the current committed Codex checkout on a Mac,
verified locally, transferred directly over SSH, and loaded into the NAS Docker
daemon.

The live Compose project and its persistent state do not belong in this source
repository. They remain in the NAS deployment repository. Rebuilding and loading
an image does not recreate the service or modify its volumes.

## Build and transfer

Run from a clean Codex checkout:

```sh
deploy/personas/runtimes/codex/build-transfer-macos.sh
```

The helper derives the image tag from the current Git commit, verifies that the
nearest release tag is an exact stable Rust release, builds `linux/amd64` with
Docker Desktop Buildx, runs package and sandbox checks, saves a compressed image
archive, verifies its checksum after transfer, and loads the immutable image on
the NAS.

These environment variables override deployment-specific defaults:

- `NAS_SSH_CONFIG`
- `NAS_SSH_HOST`
- `NAS_IMAGE_DIRECTORY`
- `CODEX_IMAGE_ARTIFACT_DIRECTORY`
- `CODEX_UID`
- `CODEX_GID`
- `CARGO_BUILD_JOBS`

The Docker build context excludes `.git`, `.codex`, authentication files,
environment files, persistent state, workspaces, shared persona data, and Rust
build output. No `CODEX_HOME` data or credentials are stored in the image.

Unless Jonathan explicitly says otherwise, source updates include NAS deployment.
After the helper succeeds, wait until the shared App Server reports all loaded
threads idle, including child threads and turns waiting for approval or input.
Recheck immediately before updating the NAS Compose project's Codex image tag and
gracefully recreating only its Codex service. Do not interrupt active chats to make
the service idle. Preserve state and enrollment, verify RPC health and reconnection,
and keep the previous immutable image for rollback.
