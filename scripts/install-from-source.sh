#!/bin/sh

set -eu

skip_build=false
after_tui_shutdown=false

usage() {
  cat <<EOF
Usage: install-from-source.sh [--skip-build] [--after-tui-shutdown]

Build and install this checkout as the managed standalone Codex package.

Options:
  --skip-build          Package existing release binaries without rebuilding them.
  --after-tui-shutdown  Restart the managed app-server after the TUI shuts down.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --skip-build)
      skip_build=true
      ;;
    --after-tui-shutdown)
      after_tui_shutdown=true
      ;;
    --help | -h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)"
checkout_root="$(dirname "$script_dir")"
codex_root="$checkout_root/codex-rs"
codex_home_dir="${CODEX_HOME:-$HOME/.codex}"
bin_dir="${CODEX_INSTALL_DIR:-$HOME/.local/bin}"
standalone_root="$codex_home_dir/packages/standalone"
releases_dir="$standalone_root/releases"
current_link="$standalone_root/current"
daemon_state_dir="$codex_home_dir/app-server-daemon"
daemon_was_configured=false
remote_control_enabled=false
stage_dir=""
activation_stage_dir=""
previous_active_dir=""

is_descendant_of_pid() {
  ancestor_pid="$1"
  candidate_pid="$$"

  while [ "$candidate_pid" -gt 1 ]; do
    if [ "$candidate_pid" = "$ancestor_pid" ]; then
      return 0
    fi
    candidate_pid="$(ps -o ppid= -p "$candidate_pid" 2>/dev/null | tr -d '[:space:]')"
    case "$candidate_pid" in
      "" | *[!0-9]*)
        return 1
        ;;
    esac
  done

  return 1
}

case "$(uname -s):$(uname -m)" in
  Darwin:arm64 | Darwin:aarch64)
    vendor_target="aarch64-apple-darwin"
    ;;
  Darwin:x86_64)
    vendor_target="x86_64-apple-darwin"
    ;;
  Linux:arm64 | Linux:aarch64)
    vendor_target="aarch64-unknown-linux-gnu"
    ;;
  Linux:x86_64 | Linux:amd64)
    vendor_target="x86_64-unknown-linux-gnu"
    ;;
  *)
    echo "Unsupported source-install platform: $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

active_release_dir="$releases_dir/source-current-$vendor_target"

cleanup() {
  if [ -n "$stage_dir" ]; then
    rm -rf "$stage_dir"
  fi
  if [ -n "$activation_stage_dir" ]; then
    rm -rf "$activation_stage_dir"
  fi
  if [ -n "$previous_active_dir" ] &&
    { [ -e "$previous_active_dir" ] || [ -L "$previous_active_dir" ]; }; then
    if [ ! -e "$active_release_dir" ]; then
      mv "$previous_active_dir" "$active_release_dir"
    else
      rm -rf "$previous_active_dir"
    fi
  fi
}
trap cleanup EXIT INT TERM

target_dir="$codex_root/target/$vendor_target/release"
codex_bin="$target_dir/codex"
code_mode_host_bin="$target_dir/codex-code-mode-host"

if [ -f "$daemon_state_dir/settings.json" ] || [ -f "$daemon_state_dir/app-server.pid" ]; then
  daemon_was_configured=true
fi
if [ -f "$daemon_state_dir/settings.json" ] &&
  grep -Eq '"remoteControlEnabled"[[:space:]]*:[[:space:]]*true' "$daemon_state_dir/settings.json"; then
  remote_control_enabled=true
fi

daemon_pid=""
if [ -f "$daemon_state_dir/app-server.pid" ]; then
  daemon_pid="$(
    awk '
      match($0, /"pid"[[:space:]]*:[[:space:]]*[0-9]+/) {
        value = substr($0, RSTART, RLENGTH)
        sub(/^.*:/, "", value)
        gsub(/[[:space:]]/, "", value)
        print value
        exit
      }
    ' "$daemon_state_dir/app-server.pid"
  )"
fi

if [ "$daemon_was_configured" = true ] &&
  [ "$after_tui_shutdown" = false ] &&
  { [ -n "${CODEX_THREAD_ID:-}" ] ||
    [ -n "${CODEX_SESSION_ID:-}" ] ||
    { [ -n "$daemon_pid" ] && is_descendant_of_pid "$daemon_pid"; }; }; then
  echo "Refusing to restart the managed app-server from an active Codex turn." >&2
  echo 'Use $update-codex, or run this installer from a terminal after Codex exits.' >&2
  exit 1
fi

if [ "$skip_build" = false ]; then
  echo "==> Building Codex release binaries"
  python3 "$checkout_root/scripts/build_codex_source.py" \
    --target "$vendor_target" \
    --profile release
fi

if [ ! -x "$codex_bin" ] || [ ! -x "$code_mode_host_bin" ]; then
  echo "Release binaries are not available under $target_dir." >&2
  echo "Run without --skip-build to build them first." >&2
  exit 1
fi

if [ "$(uname -s)" = Darwin ]; then
  echo "==> Signing Codex release binaries"
  "$checkout_root/scripts/codex-source-signing.sh" \
    "$codex_bin" \
    com.openai.codex.source \
    "$code_mode_host_bin" \
    com.openai.codex.source.code-mode-host
fi

version="$($codex_bin --version | awk 'NR == 1 { print $2 }')"
if [ -z "$version" ]; then
  echo "Could not read the Codex version from $codex_bin." >&2
  exit 1
fi

if command -v shasum >/dev/null 2>&1; then
  binary_digest="$(
    shasum -a 256 "$codex_bin" "$code_mode_host_bin" |
      shasum -a 256 |
      awk 'NR == 1 { print substr($1, 1, 12) }'
  )"
else
  binary_digest="$(
    sha256sum "$codex_bin" "$code_mode_host_bin" |
      sha256sum |
      awk 'NR == 1 { print substr($1, 1, 12) }'
  )"
fi
release_dir="$releases_dir/$version-source-$binary_digest-$vendor_target"

mkdir -p "$releases_dir" "$bin_dir"

echo "==> Stopping the official standalone updater"
"$codex_bin" app-server daemon stop-updater >/dev/null

if [ ! -x "$release_dir/bin/codex" ] ||
  [ ! -x "$release_dir/bin/codex-code-mode-host" ] ||
  [ ! -x "$release_dir/codex" ] ||
  [ ! -x "$release_dir/codex-path/rg" ] ||
  [ ! -x "$release_dir/codex-resources/zsh/bin/zsh" ] ||
  [ ! -f "$release_dir/codex-package.json" ]; then
  if [ -e "$release_dir" ] || [ -L "$release_dir" ]; then
    rm -rf "$release_dir"
  fi

  stage_dir="$(mktemp -d "$standalone_root/.source-install.XXXXXX")"

  set -- \
    --target "$vendor_target" \
    --variant codex \
    --package-version "$version" \
    --package-dir "$stage_dir/package" \
    --entrypoint-bin "$codex_bin" \
    --code-mode-host-bin "$code_mode_host_bin"

  if [ -x "$current_link/codex-path/rg" ]; then
    set -- "$@" --rg-bin "$current_link/codex-path/rg"
  fi
  if [ -x "$current_link/codex-resources/zsh/bin/zsh" ]; then
    set -- "$@" --zsh-bin "$current_link/codex-resources/zsh/bin/zsh"
  fi

  echo "==> Assembling the standalone package"
  (
    cd "$checkout_root"
    CODEX_REPO_ROOT="$checkout_root" python3 scripts/build_codex_package.py "$@"
  )
  if [ ! -e "$stage_dir/package/codex" ]; then
    ln -s bin/codex "$stage_dir/package/codex"
  fi
  mv "$stage_dir/package" "$release_dir"
fi

replace_with_symlink() {
  link_path="$1"
  link_target="$2"
  temp_link="$link_path.source-install.$$"
  rm -f "$temp_link"
  ln -s "$link_target" "$temp_link"
  if [ "$(uname -s)" = Darwin ]; then
    mv -fh "$temp_link" "$link_path"
  else
    mv -fT "$temp_link" "$link_path"
  fi
}

echo "==> Installing the stable source package at $active_release_dir"
activation_stage_dir="$(mktemp -d "$standalone_root/.source-activate.XXXXXX")"
mkdir "$activation_stage_dir/package"
cp -R "$release_dir/." "$activation_stage_dir/package/"

if [ -e "$active_release_dir" ] || [ -L "$active_release_dir" ]; then
  previous_active_dir="$standalone_root/.source-previous.$$"
  mv "$active_release_dir" "$previous_active_dir"
fi
mv "$activation_stage_dir/package" "$active_release_dir"
rm -rf "$activation_stage_dir"
activation_stage_dir=""
if [ -n "$previous_active_dir" ]; then
  rm -rf "$previous_active_dir"
  previous_active_dir=""
fi

echo "==> Activating $active_release_dir"
replace_with_symlink "$current_link" "$active_release_dir"
replace_with_symlink "$bin_dir/codex" "$current_link/bin/codex"
replace_with_symlink "$bin_dir/codex-code-mode-host" "$current_link/bin/codex-code-mode-host"

if [ "$(uname -s)" = Darwin ] &&
  [ -f "$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.openai.codexextension.json" ]; then
  echo "==> Installing the signed Codex Node REPL launcher"
  "$checkout_root/scripts/codex-node-repl-launcher.mjs" --install
  echo "==> Installing the Codex Chrome native-host compatibility proxy"
  "$checkout_root/scripts/codex-chrome-native-host-proxy.mjs" --install
fi

if [ "$daemon_was_configured" = true ]; then
  echo "==> Restarting the managed app-server"
  if [ "$remote_control_enabled" = true ]; then
    "$bin_dir/codex" app-server daemon bootstrap --remote-control >/dev/null
  else
    "$bin_dir/codex" app-server daemon bootstrap >/dev/null
  fi
fi

"$bin_dir/codex" --version
echo "Source-built Codex installed successfully."
