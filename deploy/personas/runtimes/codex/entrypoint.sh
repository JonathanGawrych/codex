#!/bin/bash
set -euo pipefail

case "$(stat -f -c %T "$CODEX_HOME")" in
    cifs | nfs | nfs4 | smb2)
        echo "Refusing to run Codex state on a network filesystem: $CODEX_HOME" >&2
        exit 1
        ;;
esac

if [[ ! -w "$CODEX_HOME" ]]; then
    echo "CODEX_HOME is not writable: $CODEX_HOME" >&2
    exit 1
fi

umask 077
exec /opt/codex/bin/codex "$@"
