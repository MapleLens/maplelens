"""Release step: `python scripts/sign_release.py maplelens-vX.Y.Z-windows.zip`.

Writes the signed manifest (`...-manifest.json` plus `.sig`) next to the zip. The private key comes from the
UPDATE_SIGNING_KEY environment variable (a CI secret); the result is verified against the public key built into
the app, so a wrong or rotated secret fails the release instead of shipping updates nobody would accept.
"""

import base64
import hashlib
import json
import os
import sys
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from maplelens.update.manifest import asset_name, manifest_name, verify_manifest
from maplelens.update.version import format_version, parse_version


def main(zip_path: Path) -> int:
    key_text = os.environ.get("UPDATE_SIGNING_KEY", "").strip()
    if not key_text:
        print("UPDATE_SIGNING_KEY is not set")
        return 1
    version = format_version(parse_version(zip_path.name.removeprefix("maplelens-").removesuffix("-windows.zip")))
    if zip_path.name != asset_name(version):
        print(f"unexpected asset name {zip_path.name}")
        return 1
    data = zip_path.read_bytes()
    manifest = json.dumps({"version": version, "asset": zip_path.name, "size": len(data),
                           "sha256": hashlib.sha256(data).hexdigest()}, indent=2).encode("ascii")
    signature = base64.b64encode(Ed25519PrivateKey.from_private_bytes(base64.b64decode(key_text)).sign(manifest))
    verify_manifest(manifest, signature)   # raises unless the app's built-in public key accepts it
    out = zip_path.with_name(manifest_name(version))
    out.write_bytes(manifest)
    out.with_name(out.name + ".sig").write_bytes(signature)
    print(f"signed {out.name} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(Path(sys.argv[1])))
