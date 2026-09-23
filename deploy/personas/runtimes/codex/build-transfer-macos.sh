#!/bin/bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repository_root="${CODEX_SOURCE_DIR:-$(cd "$script_dir/../../../.." && pwd)}"
source_commit="$(git -C "$repository_root" rev-parse --short=12 HEAD)"
source_tag="$(git -C "$repository_root" describe --tags --match 'rust-v[0-9]*' --abbrev=0 HEAD)"
expected_version="${source_tag#rust-v}"
image="codex-personas:git-${source_commit}-synology"

nas_ssh_config="${NAS_SSH_CONFIG:-$HOME/.claude/projects/-Volumes-docker-gawrych-server/memory/nas_ssh_config}"
nas_ssh_host="${NAS_SSH_HOST:-nas}"
nas_image_directory="${NAS_IMAGE_DIRECTORY:-/volume2/docker/personas/images}"
artifact_directory="${CODEX_IMAGE_ARTIFACT_DIRECTORY:-$HOME/.codex/build-artifacts/personas}"
archive="$artifact_directory/codex-personas-${source_commit}-linux-amd64.tar.gz"
remote_archive="$nas_image_directory/$(basename "$archive")"

codex_uid="${CODEX_UID:-1026}"
codex_gid="${CODEX_GID:-100}"
cargo_build_jobs="${CARGO_BUILD_JOBS:-2}"

case "$source_commit" in
    *[!0-9a-f]* | "")
        echo "invalid source commit: $source_commit" >&2
        exit 1
        ;;
esac

if [[ ! "$source_tag" =~ ^rust-v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "nearest source tag is not an exact stable release: $source_tag" >&2
    exit 1
fi

case "$nas_image_directory" in
    *[!A-Za-z0-9._/-]* | "")
        echo "invalid NAS image directory: $nas_image_directory" >&2
        exit 1
        ;;
esac

if [[ ! -f "$nas_ssh_config" ]]; then
    echo "SSH configuration does not exist: $nas_ssh_config" >&2
    exit 1
fi

if [[ -e "$repository_root/deploy/personas/.env" ]]; then
    echo "Refusing to build with deploy/personas/.env present in the checkout." >&2
    exit 1
fi

if ! git -C "$repository_root" diff --quiet \
    || ! git -C "$repository_root" diff --cached --quiet; then
    echo "Refusing to build a source image with uncommitted tracked changes." >&2
    exit 1
fi

for required_exclusion in '.codex' '**/.env' '**/auth.json' '**/target' 'deploy/personas/state'; do
    if ! grep -Fqx "$required_exclusion" "$script_dir/Dockerfile.dockerignore"; then
        echo "Dockerfile.dockerignore does not exclude $required_exclusion" >&2
        exit 1
    fi
done

mkdir -p "$artifact_directory"
chmod 0700 "$artifact_directory"
available_kib="$(df -Pk "$artifact_directory" | awk 'NR == 2 { print $4 }')"
minimum_free_kib="$((20 * 1024 * 1024))"
if [[ ! "$available_kib" =~ ^[0-9]+$ ]] || (( available_kib < minimum_free_kib )); then
    echo "At least 20 GiB of free disk space is required for the Mac image build." >&2
    exit 1
fi

build_started_at="$(date +%s)"
docker buildx build \
    --builder desktop-linux \
    --platform linux/amd64 \
    --progress plain \
    --load \
    --file "$script_dir/Dockerfile" \
    --build-arg CODEX_BWRAP_SUPPORT_SETUID=1 \
    --build-arg CODEX_GID="$codex_gid" \
    --build-arg CODEX_LINUX_SANDBOX_DISABLE_SECCOMP=1 \
    --build-arg CODEX_TARGET_ARCH=amd64 \
    --build-arg CODEX_UID="$codex_uid" \
    --build-arg CARGO_BUILD_JOBS="$cargo_build_jobs" \
    --build-arg SOURCE_COMMIT="$source_commit" \
    --tag "$image" \
    "$repository_root"
build_duration_seconds="$(( $(date +%s) - build_started_at ))"

image_architecture="$(docker image inspect --format '{{.Architecture}}' "$image")"
image_revision="$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$image")"
if [[ "$image_architecture" != "amd64" ]]; then
    echo "expected amd64 image, got $image_architecture" >&2
    exit 1
fi
if [[ "$image_revision" != "$source_commit" ]]; then
    echo "expected source revision $source_commit, got $image_revision" >&2
    exit 1
fi

docker run --rm --platform linux/amd64 --network none --read-only \
    --env EXPECTED_GID="$codex_gid" \
    --env EXPECTED_UID="$codex_uid" \
    --entrypoint /bin/sh "$image" -c '
        set -eu
        test "$(id -u)" -eq "$EXPECTED_UID"
        test "$(id -g)" -eq "$EXPECTED_GID"
        test "$(command -v bwrap)" = /usr/local/bin/bwrap
        test "$(readlink /usr/local/bin/bwrap)" = /opt/codex/bin/bwrap-setuid-wrapper
        test -x /opt/codex/bin/codex-statusline
        test -x /opt/personas/codex/entrypoint.sh
        test "$(readlink /home/codex/Documents)" = /workspaces
        test "$(readlink /workspace/secretary)" = /workspaces/secretary
    '

docker run --rm --platform linux/amd64 --network none --read-only --user 0 \
    --entrypoint /bin/sh "$image" -c '
        set -eu
        test "$(stat -c %U:%G:%a /opt/codex/codex-resources/bwrap)" = root:root:4755
        test "$(find / -xdev -type f -perm /6000 | wc -l)" -eq 1
        test "$(find / -xdev -type f -perm /6000)" = /opt/codex/codex-resources/bwrap
        test -z "$(getcap -r / 2>/dev/null)"
    '

codex_version="$(docker run --rm --platform linux/amd64 --network none --read-only \
    --entrypoint /opt/codex/bin/codex "$image" --version)"
node_version="$(docker run --rm --platform linux/amd64 --network none --read-only \
    --entrypoint /usr/local/bin/node "$image" --version)"
simplenote_version="$(docker run --rm --platform linux/amd64 --network none --read-only \
    --entrypoint /usr/local/bin/node "$image" \
    -p "require('/opt/node-tools/node_modules/@automattic/simplenote-mcp/package.json').version")"
if [[ "$codex_version" != "codex-cli $expected_version" ]]; then
    echo "unexpected Codex version: $codex_version" >&2
    exit 1
fi
if [[ ! "$node_version" =~ ^v2[2-9]\. || "$simplenote_version" != "2.0.1" ]]; then
    echo "Node or Simplenote MCP verification failed" >&2
    exit 1
fi

probe_root="$(mktemp -d "${TMPDIR:-/tmp}/codex-personas-sandbox.XXXXXX")"
cleanup_probe() {
    rm -rf -- "$probe_root"
}
trap cleanup_probe EXIT HUP INT TERM
mkdir "$probe_root/workspaces" "$probe_root/outside"

run_sandbox() {
    docker run --rm \
        --platform linux/amd64 \
        --network none \
        --read-only \
        --tmpfs "/home/codex/.codex:rw,nosuid,nodev,size=64m,uid=$codex_uid,gid=$codex_gid,mode=0700" \
        --tmpfs /tmp:rw,nosuid,nodev,size=64m,mode=1777 \
        --cap-drop ALL \
        --cap-add NET_ADMIN \
        --cap-add SETGID \
        --cap-add SETUID \
        --cap-add SYS_ADMIN \
        --cap-add SYS_CHROOT \
        --cap-add SYS_PTRACE \
        --security-opt no-new-privileges:false \
        --security-opt seccomp=unconfined \
        --mount "type=bind,src=$probe_root/workspaces,dst=/workspaces" \
        --mount "type=bind,src=$probe_root/outside,dst=/outside" \
        "$image" "$@"
}

run_sandbox sandbox -P :read-only -C /workspaces /bin/true
run_sandbox sandbox -P :workspace -C /workspaces /bin/true
run_sandbox sandbox -P :danger-full-access -C /workspaces /bin/true

if run_sandbox sandbox -P :read-only -C /workspaces \
    /usr/bin/touch /workspaces/.codex-sandbox-probe; then
    echo "read-only sandbox unexpectedly wrote to the workspace" >&2
    exit 1
fi

run_sandbox sandbox -P :workspace -C /workspaces \
    /usr/bin/touch /workspaces/.codex-sandbox-probe
test -f "$probe_root/workspaces/.codex-sandbox-probe"
rm "$probe_root/workspaces/.codex-sandbox-probe"

if run_sandbox sandbox -P :workspace -C /workspaces \
    /usr/bin/touch /outside/.codex-sandbox-probe; then
    echo "workspace sandbox unexpectedly wrote outside the workspace" >&2
    exit 1
fi

run_sandbox sandbox -P :workspace -C /workspaces \
    /bin/sh -c 'test "$1" = "--unshare-user"' shell --unshare-user

capabilities="$(run_sandbox sandbox -P :workspace -C /workspaces \
    /bin/sh -c 'grep -E "^Cap(Eff|Prm):" /proc/self/status')"
expected_capabilities="$(printf 'CapPrm:\t0000000000000000\nCapEff:\t0000000000000000')"
if [[ "$capabilities" != "$expected_capabilities" ]]; then
    echo "sandboxed process retained effective or permitted capabilities" >&2
    exit 1
fi

trap - EXIT HUP INT TERM
cleanup_probe

archive_started_at="$(date +%s)"
docker image save "$image" | gzip -1 > "$archive"
archive_duration_seconds="$(( $(date +%s) - archive_started_at ))"
chmod 0600 "$archive"
archive_size_bytes="$(stat -f %z "$archive")"
archive_checksum="$(shasum -a 256 "$archive" | awk '{print $1}')"
local_image_id="$(docker image inspect --format '{{.Id}}' "$image")"
archive_config_path="$(gzip -dc "$archive" | tar -xOf - manifest.json | jq -er '
    if length == 1 then .[0].Config else error("expected one image in archive") end
')"
if [[ ! "$archive_config_path" =~ ^blobs/sha256/[0-9a-f]{64}$ ]]; then
    echo "unexpected image config path in archive: $archive_config_path" >&2
    exit 1
fi
archive_image_id="sha256:${archive_config_path##*/}"

ssh_options=(-o BatchMode=yes -F "$nas_ssh_config")
transfer_started_at="$(date +%s)"
ssh "${ssh_options[@]}" "$nas_ssh_host" \
    "umask 077; mkdir -p '$nas_image_directory'; cat > '$remote_archive.partial'; mv '$remote_archive.partial' '$remote_archive'" \
    < "$archive"
transfer_duration_seconds="$(( $(date +%s) - transfer_started_at ))"

remote_checksum="$(ssh "${ssh_options[@]}" "$nas_ssh_host" "sha256sum '$remote_archive' | awk '{print \$1}'")"
if [[ "$remote_checksum" != "$archive_checksum" ]]; then
    echo "NAS archive checksum does not match the Mac archive" >&2
    exit 1
fi

load_started_at="$(date +%s)"
ssh "${ssh_options[@]}" "$nas_ssh_host" \
    "gzip -dc '$remote_archive' | sudo env PATH=/usr/local/bin:/usr/bin:/bin docker image load"
load_duration_seconds="$(( $(date +%s) - load_started_at ))"

remote_image="$(ssh "${ssh_options[@]}" "$nas_ssh_host" \
    "sudo env PATH=/usr/local/bin:/usr/bin:/bin docker image inspect --format '{{.Id}} {{.Architecture}}' '$image'")"
if [[ "$remote_image" != "$archive_image_id amd64" ]]; then
    echo "NAS image verification failed: $remote_image" >&2
    exit 1
fi

printf '%s\n' \
    "image=$image" \
    "local_image_index_id=$local_image_id" \
    "archive_image_id=$archive_image_id" \
    "remote_image_id=${remote_image%% *}" \
    "architecture=$image_architecture" \
    "codex_version=$codex_version" \
    "node_version=$node_version" \
    "simplenote_mcp_version=$simplenote_version" \
    "build_duration_seconds=$build_duration_seconds" \
    "archive_duration_seconds=$archive_duration_seconds" \
    "archive_size_bytes=$archive_size_bytes" \
    "archive_checksum=$archive_checksum" \
    "transfer_duration_seconds=$transfer_duration_seconds" \
    "load_duration_seconds=$load_duration_seconds" \
    "remote_archive=$remote_archive"
