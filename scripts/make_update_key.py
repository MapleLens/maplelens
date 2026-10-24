"""One-time setup of the update signing key: `python scripts/make_update_key.py --out PATH`.

Writes the private key to PATH (a backup for you: refuses to overwrite), stores it as the GitHub Actions secret
UPDATE_SIGNING_KEY through the `gh` CLI (stdin, never argv), and prints the public key to paste into
`config.UPDATE_PUBLIC_KEY`. The private key is never printed and must never be committed.

Rotating the key strands every installed copy, which only trusts the public key it shipped with.
"""

import argparse
import base64
import os
import subprocess
import sys
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

SECRET_NAME = "UPDATE_SIGNING_KEY"


def _b64(raw: bytes) -> str:
    return base64.b64encode(raw).decode("ascii")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, required=True, help="where to back up the private key (outside the repo)")
    parser.add_argument("--repo", default="MapleLens/maplelens", help="owner/name that receives the secret")
    parser.add_argument("--no-secret", action="store_true", help="skip storing the GitHub secret")
    args = parser.parse_args()

    key = Ed25519PrivateKey.generate()
    private = _b64(key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw,
                                     serialization.NoEncryption()))
    public = _b64(key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw))

    args.out.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(args.out, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)   # EXCL: never clobber an older key
    with os.fdopen(fd, "w", encoding="ascii") as backup:
        backup.write(private + "\n")
    if not args.no_secret:
        done = subprocess.run(["gh", "secret", "set", SECRET_NAME, "--repo", args.repo], input=private,
                              text=True, check=False)
        if done.returncode:
            print(f"gh could not store the secret; the key is saved in {args.out}", file=sys.stderr)
            return done.returncode
    print(f"private key backed up to {args.out}\npublic key: {public}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
