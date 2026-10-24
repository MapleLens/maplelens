"""The signed release manifest: what a downloaded zip must be, vouched for by a key baked into the app.

A hash published beside the zip only catches a corrupt download; anyone able to swap the zip could swap the hash.
The manifest is signed with an Ed25519 key whose public half ships in `config`, so it holds up even if the
download host is compromised.
"""

import base64
import binascii
import json
import re
from dataclasses import dataclass

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from maplelens import config
from maplelens.update import UpdateError
from maplelens.update.version import format_version, parse_version

_SHA256 = re.compile(r"[0-9a-f]{64}")


def asset_name(version: str) -> str:
    return f"maplelens-v{version}-windows.zip"


def manifest_name(version: str) -> str:
    return f"maplelens-v{version}-manifest.json"


@dataclass(frozen=True)
class Manifest:
    version: str
    asset: str
    size: int
    sha256: str


def verify_manifest(data: bytes, signature: bytes, public_key: str = config.UPDATE_PUBLIC_KEY) -> Manifest:
    """Check the signature over the exact bytes first; only then is the content parsed."""
    try:
        key = Ed25519PublicKey.from_public_bytes(base64.b64decode(public_key, validate=True))
        key.verify(base64.b64decode(signature.strip(), validate=True), data)
    except (InvalidSignature, binascii.Error, ValueError) as exc:
        raise UpdateError("the update's signature is not valid") from exc
    try:
        raw = json.loads(data)
        version = format_version(parse_version(raw["version"]))
        manifest = Manifest(version, raw["asset"], raw["size"], raw["sha256"])
    except (ValueError, KeyError, TypeError) as exc:
        raise UpdateError("the update manifest is malformed") from exc
    ok = (manifest.asset == asset_name(version) and isinstance(manifest.size, int)
          and not isinstance(manifest.size, bool) and 0 < manifest.size <= config.UPDATE_MAX_ZIP_BYTES
          and isinstance(manifest.sha256, str) and _SHA256.fullmatch(manifest.sha256))
    if not ok:
        raise UpdateError("the update manifest is malformed")
    return manifest
