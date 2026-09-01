# NAS-hosted Codex secretary

This deployment runs one Codex App Server as the container's long-lived process. The App Server listens only on a Unix socket inside the persistent `CODEX_HOME` volume. Remote Control makes outbound HTTPS and WebSocket connections to the OpenAI relay. Compose publishes no TCP ports and does not mount the Docker socket. The container root remains read-only; `CODEX_HOME`, the Secretary workspace, and Remote's `/home/codex/Documents` project directory are the persistent writable mounts. `/tmp` is a temporary writable `tmpfs`.

This directory contains no authentication secrets. `.env` contains deployment settings, is ignored by Git, and is excluded from the Docker build context. Codex stores authentication, the installation identity, Remote Control enrollment, and session data under `CODEX_HOME`. Backups of that volume contain credentials and must be protected like credentials.

The [public Remote documentation](https://learn.chatgpt.com/docs/remote) currently lists the ChatGPT desktop app on macOS and Windows as supported hosts. This Codex fork contains an experimental CLI path that sends `os: "linux"` during relay enrollment. The exact Synology deployment documented here completed manual pairing and the full phone and restart acceptance test with the official phone Remote interface on 2026-09-01. This verifies the current custom image and account, not general OpenAI support for Linux hosts. See [ACCEPTANCE.md](./ACCEPTANCE.md) for the recorded evidence.

## Architecture

The Dockerfile builds natively for `linux/amd64` and `linux/arm64`. On the NAS, confirm its architecture:

```sh
uname -m
docker info --format '{{.Architecture}}'
```

`x86_64` or `amd64` uses the `x86_64-unknown-linux-gnu` Codex target. `aarch64` or `arm64` uses `aarch64-unknown-linux-gnu`. A normal `docker compose build` selects the NAS architecture automatically.

The deployment requires Docker Compose v2. BuildKit is optional. Synology installations vary, so record these versions before deployment:

```sh
docker version
docker compose version
docker buildx version
```

Set `CODEX_TARGET_ARCH` in `.env` to `amd64` or `arm64` from the Docker architecture output. BuildKit sets `TARGETARCH` automatically when it is available. The explicit value lets Synology installations without Buildx use the legacy builder:

```sh
DOCKER_BUILDKIT=0 docker compose build secretary
```

The Container Manager graphical interface may not preserve the relative seccomp profile path or the external volume declaration. Use the Compose CLI over SSH unless the installed Container Manager version is confirmed to apply both settings.

The Rust and Debian base images are pinned by multi-architecture digest. The build uses this checkout's source and Rust `1.95.0`, then creates the repository's canonical Codex package with the CLI, code-mode host, Linux sandbox helper, patched zsh, and ripgrep.

`CARGO_BUILD_JOBS=1` limits peak linker memory for small NAS hosts. Increase it only when the builder has enough memory for concurrent release links.

## Linux sandbox prerequisite

The Compose service drops every Linux capability, then adds only the six capabilities Bubblewrap requires to its bounding set: `NET_ADMIN`, `SETGID`, `SETUID`, `SYS_ADMIN`, `SYS_CHROOT`, and `SYS_PTRACE`. The default `CODEX_NO_NEW_PRIVILEGES=true` prevents the non-root service and its children from acquiring those capabilities. Docker's default seccomp profile blocks the namespace and mount calls required by Codex's `bwrap` sandbox, so Compose applies `bubblewrap-seccomp.json` instead.

That profile starts with Moby's default profile at revision `61eaf32614c7c71b60bd8927d3e6a4ffc8ff1f31`. It additionally permits three exact `clone` namespace flag combinations used by Codex's readiness probe and sandbox commands, the exact second-level `CLONE_NEWUSER` call used for device setup, and `mount`, `pivot_root`, `umount`, and `umount2`. Those mount calls remain subject to the kernel's capability checks. The container has no capabilities in its initial user namespace, while `bwrap` receives mount capability only inside the unprivileged user and mount namespaces it creates for each sandboxed command.

Some Synology kernels are built without seccomp. Docker then rejects every custom seccomp profile with `seccomp is not enabled in your kernel`. Confirm that exact error with the long-lived service stopped, then set this host-specific value in `.env`:

```sh
CODEX_SECCOMP_SECURITY_OPT=seccomp=unconfined
```

Use `unconfined` only on a host whose kernel cannot provide seccomp. The non-root UID, read-only root filesystem, and `no-new-privileges` remain active. The sandbox verification commands below must still pass before the service is used.

Do not add `privileged: true` or mount the Docker socket.

The NAS kernel must permit unprivileged user namespaces. Inspect both settings before deployment. `kernel.unprivileged_userns_clone` is not present on every kernel; when it is present, it must not be `0`. `user.max_user_namespaces` must be greater than `0`.

```sh
sysctl user.max_user_namespaces
sysctl kernel.unprivileged_userns_clone 2>/dev/null || true
```

### Host-specific setuid fallback

Some Synology kernels do not provide unprivileged user namespaces. On those hosts, the sandbox commands fail with `Creating new namespace failed` even when Docker accepts the custom seccomp profile. Confirm that failure before enabling this fallback.

Set these values in `.env`:

```sh
CODEX_BWRAP_SUPPORT_SETUID=1
CODEX_LINUX_SANDBOX_DISABLE_SECCOMP=1
CODEX_NO_NEW_PRIVILEGES=false
CODEX_APPARMOR_PROFILE=unconfined
CODEX_SECCOMP_SECURITY_OPT=seccomp=unconfined
```

Rebuild the image after changing `CODEX_BWRAP_SUPPORT_SETUID`. The build compiles Bubblewrap with its upstream setuid support, removes setuid and setgid mode bits from every other file in the image, and installs only `/opt/codex/codex-resources/bwrap` as root-owned mode `4755`. A root-owned wrapper removes Codex's explicit `--unshare-user` option before executing Bubblewrap because setuid Bubblewrap uses the host's initial user namespace. Arguments after Bubblewrap's `--` command separator are preserved unchanged.

The Synology Docker profile blocks the mount propagation setup required by setuid Bubblewrap, so this fallback also disables AppArmor for this one container. The NAS kernel described above has no seccomp support. `CODEX_SECCOMP_SECURITY_OPT=seccomp=unconfined` disables Docker's filter, while the build-time `CODEX_LINUX_SANDBOX_DISABLE_SECCOMP=1` disables Codex's internal network filter. Restricted commands still run in Bubblewrap's isolated network namespace. Managed proxy networking is rejected because its network restrictions require seccomp. The service still runs as the configured non-root UID, the root filesystem stays read-only, the Docker socket remains unmounted, no ports are published, and only Bubblewrap can use the six capabilities in the container's bounding set. Bubblewrap drops its effective UID and capabilities before it executes a sandboxed command, then Codex applies `no_new_privs` before the requested command starts.

Verify the image and running service before use:

```sh
docker compose run --rm --no-deps --entrypoint /bin/sh secretary -c \
  'test "$(stat -c "%U:%G:%a" /opt/codex/codex-resources/bwrap)" = "root:root:4755"; test "$(find / -xdev -type f -perm /6000 | wc -l)" -eq 1'
docker compose exec secretary /bin/sh -c \
  'grep -E "^Cap(Eff|Prm):" /proc/1/status'
```

Both `CapEff` and `CapPrm` for the App Server process must be `0000000000000000`. The sandbox verification commands below must also pass.

The custom TUI modes use these filesystem profiles:

- Manual uses `:read-only`.
- Accept Edits and Auto use `:workspace`. Auto changes the approval reviewer, not the filesystem profile.
- Bypass uses `:danger-full-access`, so the Docker container is its outer filesystem boundary.

After starting the service, verify that both sandboxed profiles can launch:

```sh
docker compose exec secretary codex sandbox -P :read-only -C /workspace/secretary /bin/true
docker compose exec secretary codex sandbox -P :workspace -C /workspace/secretary /bin/true
```

## Configure persistent storage

Run these commands from the repository root:

```sh
cd deploy/nas-secretary
cp .env.example .env
```

Set `CODEX_IMAGE_TAG` to `git-` followed by the current 12-character source commit. `.env` does not evaluate shell expressions, so copy the result of this command into the file:

```sh
git rev-parse --short=12 HEAD
```

Keep `CODEX_HOME_VOLUME` unchanged across container and image replacement. Docker manages this named volume on its Linux filesystem, which supports the App Server's Unix socket. The volume contains `auth.json`, `installation_id`, the SQLite state database, session history, configuration, skills, plugins, and Remote Control enrollment.

Edit `.env` and set the absolute secretary workspace and Remote project paths, the UID and GID that own them, and the secretary timezone. Remote uses `/home/codex/Documents` as the parent directory for new projects on this Linux host. The root filesystem remains read-only; Compose mounts only that directory as writable. Replace `1000:1000` if `.env` uses other values:

```sh
docker volume create codex-secretary-home
sudo mkdir -p /srv/codex-secretary/workspace
sudo mkdir -p /srv/codex-secretary/documents
sudo chown 1000:1000 /srv/codex-secretary/workspace
sudo chown 1000:1000 /srv/codex-secretary/documents
sudo chmod 0750 /srv/codex-secretary/workspace
sudo chmod 0750 /srv/codex-secretary/documents
```

The Codex volume is external to the Compose project, so `docker compose down --volumes` does not delete it. The installation identity is stored in this volume. The fixed Compose hostname keeps the Remote Control server name stable. Do not delete `CODEX_HOME_VOLUME` unless you intend to remove that identity and all saved sessions.

The image creates the `codex` account with the numeric `CODEX_UID` and `CODEX_GID` build arguments. A new empty named volume inherits that ownership. An existing volume retains its numeric ownership when the image is rebuilt. If either number changes, rebuild the image, stop the service, and repair both mounts before starting it. Replace `1000:1000` with the configured values:

```sh
docker compose build secretary
docker compose stop secretary
docker compose run --rm --no-deps --user root --cap-add CHOWN --cap-add DAC_OVERRIDE --entrypoint /bin/chown secretary -R 1000:1000 /home/codex/.codex
sudo chown -R 1000:1000 /srv/codex-secretary/workspace
sudo chown -R 1000:1000 /srv/codex-secretary/documents
docker compose up -d secretary
```

`CHOWN` and `DAC_OVERRIDE` are granted only to this stopped-service maintenance container. The long-lived service still has no Linux capabilities.

Synology shared-folder ACLs can still deny access even when numeric ownership is correct. The workspace and Remote project paths must grant the configured UID and GID read and write access through both their Unix mode bits and any Synology ACL.

## Build on macOS and transfer to the NAS

Build the NAS image on an Apple silicon Mac with Docker Desktop instead of compiling Rust on the NAS. The script builds the exact current checkout for `linux/amd64`, verifies the packaged image and all permission profiles, saves and checksums the image, transfers the compressed archive directly over SSH, verifies the NAS copy, and loads it into the NAS Docker daemon:

```sh
cd deploy/nas-secretary
NAS_SSH_CONFIG="$HOME/.claude/projects/-Volumes-docker-gawrych-server/memory/nas_ssh_config" \
  ./build-transfer-macos.sh
```

The script never reads or transfers `auth.json`, `CODEX_HOME`, saved sessions, SQLite state, the Secretary workspace, or Remote project files. `Dockerfile.dockerignore` excludes those names, `.env`, `.git`, `.codex`, and Rust build outputs from the build context. The saved archive contains only the image filesystem and image metadata. It is written under `$HOME/.codex/build-artifacts/nas-secretary` with mode `0600`, then streamed over SSH to `/volume2/docker/codex-secretary/images`. It does not pass through the SMB mount.

The image is tagged `codex-secretary:git-SOURCE_COMMIT-synology`. Existing source-tagged images are retained for rollback. After the script reports matching Mac and NAS image IDs, update `CODEX_IMAGE_TAG` in the NAS `.env` and recreate only the Secretary service:

```sh
docker compose up -d --no-deps --no-build secretary
docker compose ps secretary
```

The script requires at least 20 GiB of free space on the filesystem that stores its local archive. It defaults to NAS UID `1026`, GID `100`, and four Cargo build jobs. Override `CODEX_UID`, `CODEX_GID`, or `CARGO_BUILD_JOBS` in the script environment when another NAS requires different values. The NAS must not rebuild the image after loading it.

## Build directly on the NAS

Direct NAS builds remain available as a recovery path, but the DS920+ takes hours to compile this checkout and should normally load the Mac-built image instead.

## Authenticate

For a direct NAS recovery build from the current checkout:

```sh
docker compose build secretary
```

Install the minimal Secretary configuration into the clean `CODEX_HOME` volume. It selects the model, trusts only the Secretary workspace, disables automatic recaps, and enables the packaged status-line hook:

```sh
docker compose run --rm --no-deps \
  --volume "$PWD/config.toml.example:/tmp/config.toml:ro" \
  --entrypoint /bin/sh secretary \
  -c 'umask 077; cp /tmp/config.toml "$CODEX_HOME/config.toml"'
```

Use device authentication because the NAS has no local browser. Stop the App Server while `auth.json` changes, then start it again so the new process reads the credentials:

```sh
docker compose stop secretary
docker compose run --rm --no-deps secretary login --device-auth
docker compose up -d secretary
docker compose exec secretary codex login status
```

Codex stores CLI authentication in `$CODEX_HOME/auth.json` by default. The named volume preserves it without a host keyring.

Do not copy `auth.json` into `.env`, an image, or a diagnostic log. Device authentication prints its short-lived code only to the attached terminal created by `docker compose run`.

## Pair Remote Control

Manual pairing from this Linux host was accepted on 2026-09-01 at 1:50 PM MDT. A read-only App Server status request reported `connected`, and the paired-client request returned one mobile client with a recorded `lastSeenAt`. The enrollment is present in the persistent state database. The same phone completed turns in the attached TUI thread before and after an App Server restart without another pairing code.

Wait for the App Server socket, then request a short-lived manual pairing code:

```sh
docker compose ps
docker compose exec secretary codex remote-control pair
```

Enter the printed code in the official ChatGPT Remote Control pairing UI. If the command reports that server enrollment failed and identifies Linux as unsupported, stop here and keep the logs for diagnosis:

```sh
docker compose logs --tail=300 secretary
```

The relay receives outbound connections only. There is no NAS port to forward through the router.

Do not generate a pairing code until the phone is ready. The acceptance procedure below identifies the exact pairing checkpoint.

## Attach the stock TUI

Open the TUI in a separate one-off container. It shares the persistent Unix socket, but restarting the App Server service does not terminate the TUI container. `unix://` resolves to `$CODEX_HOME/app-server-control/app-server-control.sock`, the same App Server used by Remote Control:

```sh
docker compose run --rm --no-deps secretary --remote unix:// --profile tibbit -C /workspace/secretary
```

Resume the named secretary thread through that same socket:

```sh
docker compose run --rm --no-deps secretary resume --remote unix:// --profile tibbit secretary
```

Do not run `codex exec resume` for the secretary thread. That command starts an in-process App Server and competes for the thread's writer lock.

## Restart, update, and inspect

Restart the same App Server service:

```sh
docker compose restart secretary
```

Follow logs:

```sh
docker compose logs -f --tail=200 secretary
```

Compose rotates the `json-file` logs at `CODEX_LOG_MAX_SIZE` and retains `CODEX_LOG_MAX_FILES` files. The defaults are 20 MiB and five files. These limits apply to container stdout and stderr, not to saved session history in `CODEX_HOME`.

Confirm that the App Server answers an RPC through the private socket:

```sh
docker compose exec secretary codex app-server daemon version
```

The Compose health check uses the same RPC. Socket existence alone is not a readiness check because a stale socket can remain after an abnormal exit.

`restart: unless-stopped` restarts the container when the App Server process exits. Docker Compose does not restart a running container solely because its health status becomes `unhealthy`; NAS monitoring should alert on that status.

Before an update, create a stopped-service backup using the procedure below. Then update the checkout, change `CODEX_IMAGE_TAG` to the new source commit, build the new tag, and replace the service while retaining the Codex volume and both bind mounts:

```sh
docker compose build secretary
docker compose up -d secretary
docker compose ps
```

Do not reuse an old image tag for a different source commit. Keep the previous image and backup until the new service passes its health check and a TUI can resume the secretary thread. Rolling the image back is safe only when the newer Codex version did not change persistent data incompatibly. Restoring the pre-update backup into a new volume is the safe rollback when compatibility is unknown.

The App Server removes a stale Unix socket on startup. Compose uses a small init process, sends `SIGTERM`, and allows two minutes for active thread writers to close before stopping the container. Use `docker compose stop`, `restart`, or `down`. Do not use `docker kill` during normal operations.

Inspect the persistent Codex volume without printing its credential files:

```sh
docker volume inspect codex-secretary-home
```

## Back up and restore CODEX_HOME

Stop the App Server before backup so the SQLite state database and session files are mutually consistent. Run these commands from `deploy/nas-secretary`. Choose a backup directory on storage separate from the Docker volume:

```sh
set -a
. ./.env
set +a
backup_root=/srv/codex-secretary/backups
backup_name="codex-home-$(date +%Y%m%dT%H%M%S%z).tar.gz"
sudo mkdir -p "$backup_root"
sudo chown "$CODEX_UID:$CODEX_GID" "$backup_root"
sudo chmod 0700 "$backup_root"
docker compose stop secretary
docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
  --user "$CODEX_UID:$CODEX_GID" \
  --env BACKUP_NAME="$backup_name" \
  --mount "type=volume,src=$CODEX_HOME_VOLUME,dst=/codex-home,readonly" \
  --mount "type=bind,src=$backup_root,dst=/backup" \
  --entrypoint /bin/sh "codex-secretary:$CODEX_IMAGE_TAG" \
  -c 'umask 077; tar --exclude=./app-server-control -C /codex-home -czf "/backup/$BACKUP_NAME" .'
docker compose start secretary
printf '%s\n' "$backup_root/$backup_name"
```

The archive mode is `0600`. It contains `auth.json`, `installation_id`, Remote Control enrollment, saved sessions, configuration, skills, and plugins. Do not attach it to a ticket or place it in a shared directory.

This archive does not contain `SECRETARY_WORKSPACE_PATH` or `SECRETARY_DOCUMENTS_PATH`. Back up those host directories separately before they contain files that must survive a NAS storage failure.

Restore into a new volume so the current volume remains recoverable. Set `backup_root`, `backup_name`, and `restore_volume` to the selected values:

```sh
set -a
. ./.env
set +a
backup_root=/srv/codex-secretary/backups
backup_name=codex-home-REPLACE_WITH_BACKUP_NAME.tar.gz
restore_volume=codex-secretary-home-restored
docker compose stop secretary
docker volume create "$restore_volume"
docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
  --user "$CODEX_UID:$CODEX_GID" \
  --env BACKUP_NAME="$backup_name" \
  --mount "type=volume,src=$restore_volume,dst=/home/codex/.codex" \
  --mount "type=bind,src=$backup_root,dst=/backup,readonly" \
  --entrypoint /bin/sh "codex-secretary:$CODEX_IMAGE_TAG" \
  -c 'tar --no-same-owner -xzf "/backup/$BACKUP_NAME" -C /home/codex/.codex && rm -rf /home/codex/.codex/app-server-control'
```

Set `CODEX_HOME_VOLUME` in `.env` to `codex-secretary-home-restored`, then recreate the service and check readiness:

```sh
docker compose up -d --force-recreate secretary
docker compose ps
docker compose exec secretary codex app-server daemon version
```

Keep the old volume until the restored secretary thread, authentication, and Remote Control reconnection have all been verified.

## Phone pairing acceptance test

All seven steps passed on 2026-09-01. Repeat this test after a change to Remote Control, App Server reconnection, TUI subscriptions, persistent storage, or the container entrypoint.

Do not begin step 3 until Jonathan has the official phone Remote interface open and is ready to enter a short-lived code.

1. Start the service and wait until `docker compose ps` reports `healthy`:

   ```sh
   docker compose up -d secretary
   docker compose ps
   docker compose exec secretary codex login status
   ```

2. In terminal A, attach the TUI through the App Server in a separate container. Start a fresh thread, run `/rename nas-remote-check`, send `Reply with exactly: TUI READY.`, and wait for its response:

   ```sh
   docker compose run --rm --no-deps secretary --remote unix:// --profile tibbit -C /workspace/secretary
   ```

3. Pairing checkpoint: after Jonathan confirms the phone is ready, run this in terminal B and immediately enter the printed code in the official phone Remote interface:

   ```sh
   docker compose exec secretary codex remote-control pair
   ```

4. On the phone, open `nas-remote-check` and send `Reply with exactly: PHONE REMOTE CHECKPOINT.` Confirm that the user turn and response appear live in terminal A.

5. In terminal B, restart the container and wait for it to become healthy:

   ```sh
   docker compose restart secretary
   docker compose ps
   ```

6. Confirm that terminal A reconnects through the recreated socket and that the first phone turn is still present. If the one-off TUI exited instead, resume it with `docker compose run --rm --no-deps secretary resume --remote unix:// --profile tibbit nas-remote-check` and record the failure before continuing.

7. On the phone, reopen the same thread if needed and send `Reply with exactly: PHONE REMOTE RECONNECTED.` Confirm that the turn and response appear in terminal A without generating another pairing code.

## Optional cross-architecture build

To build one architecture explicitly on a BuildKit host:

```sh
docker buildx build --platform linux/amd64 --load -f deploy/nas-secretary/Dockerfile -t codex-secretary:amd64 .
docker buildx build --platform linux/arm64 --load -f deploy/nas-secretary/Dockerfile -t codex-secretary:arm64 .
```

Building an architecture different from the build host requires BuildKit emulation and takes longer than a native build.
