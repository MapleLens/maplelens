#!/usr/bin/env bash
# The release's update manifest and its signature: what the in-app updater (crates/win/src/update/manifest.rs) checks
# before it installs anything, on every player's PC, as administrator. Run by release.yml's sign job, where OpenSSL
# signs, so nothing of the build (a dependency's build script, the exe itself) ever runs beside the key.
# `maplelens --sign-release` writes the same two files.
#
#   UPDATE_SIGNING_KEY=<base64 Ed25519 key> scripts/sign-release.sh X.Y.Z   (in the folder holding the zip)
set -euo pipefail

version=${1:?usage: sign-release.sh X.Y.Z}
# Plain X.Y.Z, the only versions the updater parses (update::version): a pre-release tag fails here.
if ! [[ $version =~ ^[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}$ ]]; then
  echo "unsupported version $version" >&2
  exit 1
fi
zip="maplelens-v$version-windows.zip"
manifest="maplelens-v$version-manifest.json"
source="$(dirname "$0")/../crates/win/src/update/mod.rs"

size=$(stat -c %s "$zip")
sha256=$(sha256sum "$zip" | cut -d ' ' -f 1)
printf '{\n  "version": "%s",\n  "asset": "%s",\n  "size": %s,\n  "sha256": "%s"\n}' \
  "$version" "$zip" "$size" "$sha256" >"$manifest"

umask 077
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# The secret is the raw 32-byte key in base64 (update::release); OpenSSL reads it as PKCS#8 DER, which for an
# Ed25519 key is these 16 bytes and then the key.
{
  printf '\x30\x2e\x02\x01\x00\x30\x05\x06\x03\x2b\x65\x70\x04\x22\x04\x20'
  printf '%s' "${UPDATE_SIGNING_KEY:?UPDATE_SIGNING_KEY is not set}" | base64 -d
} >"$work/key.der"
if [ "$(stat -c %s "$work/key.der")" != 48 ]; then
  echo "UPDATE_SIGNING_KEY is not a base64 Ed25519 key" >&2
  exit 1
fi

# The app's built-in key must accept it: a wrong or rotated secret fails the release instead of shipping updates
# nobody would accept.
openssl pkey -inform DER -in "$work/key.der" -pubout -out "$work/public.pem"
public=$(openssl pkey -pubin -in "$work/public.pem" -outform DER | tail -c 32 | base64)
expected=$(sed -n 's/^pub const PUBLIC_KEY: &str = "\(.*\)";$/\1/p' "$source")
if [ -z "$expected" ] || [ "$public" != "$expected" ]; then
  echo "UPDATE_SIGNING_KEY does not match update::PUBLIC_KEY ($expected)" >&2
  exit 1
fi

openssl pkeyutl -sign -inkey "$work/key.der" -keyform DER -rawin -in "$manifest" -out "$work/signature"
openssl pkeyutl -verify -pubin -inkey "$work/public.pem" -rawin -in "$manifest" -sigfile "$work/signature" >/dev/null
base64 -w 0 "$work/signature" >"$manifest.sig"
echo "signed $manifest ($size bytes)"
