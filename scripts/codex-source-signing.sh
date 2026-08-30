#!/bin/sh

set -eu

identity_name="Codex Source Build Signing"
keychain_path="${CODEX_SOURCE_SIGNING_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)"
temp_dir=""

usage() {
  cat <<EOF
Usage: codex-source-signing.sh <binary> <identifier> [<binary> <identifier> ...]

Sign source-built Codex executables with a persistent local identity. The
source installer runs these signed executables from a persistent path so macOS
Keychain permissions continue to apply after the executables are rebuilt.
EOF
}

cleanup() {
  if [ -n "$temp_dir" ]; then
    rm -rf "$temp_dir"
  fi
}

find_identity_hash() {
  security find-identity -v -p codesigning "$keychain_path" |
    awk -v identity_name="$identity_name" '
      index($0, "\"" identity_name "\"") {
        gsub(/\"/, "", $2)
        print $2
      }
    '
}

create_identity() {
  archive_password="codex-source-signing-import"

  if security find-certificate -c "$identity_name" "$keychain_path" >/dev/null 2>&1; then
    echo "A certificate named '$identity_name' exists without a valid code-signing private key." >&2
    echo "Repair or remove that certificate in Keychain Access, then retry." >&2
    exit 1
  fi

  if ! command -v openssl >/dev/null 2>&1; then
    echo "OpenSSL is required to create the local Codex code-signing identity." >&2
    exit 1
  fi

  temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/codex-source-signing.XXXXXX")"
  trap cleanup EXIT INT TERM
  umask 077

  echo "==> Creating local code-signing identity '$identity_name'"
  openssl req \
    -new \
    -x509 \
    -newkey rsa:3072 \
    -sha256 \
    -days 3650 \
    -nodes \
    -config "$script_dir/codex-source-signing-openssl.cnf" \
    -keyout "$temp_dir/private-key.pem" \
    -out "$temp_dir/certificate.pem" >/dev/null 2>&1
  openssl pkcs12 \
    -export \
    -legacy \
    -inkey "$temp_dir/private-key.pem" \
    -in "$temp_dir/certificate.pem" \
    -out "$temp_dir/identity.p12" \
    -passout "pass:$archive_password"

  security import "$temp_dir/identity.p12" \
    -k "$keychain_path" \
    -P "$archive_password" \
    -T /usr/bin/codesign >/dev/null
  security add-trusted-cert \
    -d \
    -r trustRoot \
    -p codeSign \
    -k "$keychain_path" \
    "$temp_dir/certificate.pem"
}

sign_binary() {
  binary_path="$1"
  identifier="$2"
  external_requirement="certificate leaf = H\"$identity_hash\" and identifier \"$identifier\""
  designated_requirement="designated => $external_requirement"

  if codesign --verify --strict --test-requirement "=$external_requirement" "$binary_path" \
    >/dev/null 2>&1; then
    echo "Already signed: $binary_path"
    return
  fi

  codesign \
    --force \
    --sign "$identity_hash" \
    --identifier "$identifier" \
    --requirements "=$designated_requirement" \
    --timestamp=none \
    "$binary_path"
  codesign --verify --strict --test-requirement "=$external_requirement" "$binary_path"
}

if [ "$(uname -s)" != Darwin ]; then
  echo "codex-source-signing.sh only supports macOS." >&2
  exit 1
fi

if [ "$#" -eq 0 ] || [ $(( $# % 2 )) -ne 0 ]; then
  usage >&2
  exit 1
fi

identity_hash="$(find_identity_hash)"
if [ -z "$identity_hash" ]; then
  create_identity
  identity_hash="$(find_identity_hash)"
fi
if [ -z "$identity_hash" ]; then
  echo "Could not create a valid '$identity_name' code-signing identity." >&2
  exit 1
fi

while [ "$#" -gt 0 ]; do
  if [ ! -f "$1" ] || [ ! -x "$1" ]; then
    echo "Executable does not exist: $1" >&2
    exit 1
  fi
  sign_binary "$1" "$2"
  shift 2
done
