"""HTTPS fetching with TLS verification, a host allow-list that also covers redirects, and size caps."""

import hashlib
import urllib.error
import urllib.request
from collections.abc import Callable
from pathlib import Path
from urllib.parse import urlparse

from maplelens import __version__, config
from maplelens.update import UpdateError

# GitHub serves release downloads through a redirect to its asset CDN.
ALLOWED_HOSTS = frozenset({"api.github.com", "github.com", "objects.githubusercontent.com",
                           "release-assets.githubusercontent.com"})
_CHUNK = 1 << 16
Progress = Callable[[int, int], None]   # (bytes done, bytes expected)


def _check_url(url: str) -> None:
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname not in ALLOWED_HOSTS:
        raise UpdateError(f"refusing to fetch {parsed.hostname or url}")


class _AllowListRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        _check_url(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


_opener = urllib.request.build_opener(_AllowListRedirects)


def _open(url: str):
    _check_url(url)
    request = urllib.request.Request(url, headers={"User-Agent": f"MapleLens/{__version__}",
                                                   "Accept": "application/octet-stream, application/json"})
    try:
        return _opener.open(request, timeout=config.UPDATE_TIMEOUT_S)
    except (urllib.error.URLError, OSError) as exc:
        raise UpdateError(f"could not reach {urlparse(url).hostname}: {exc}") from exc


def get_bytes(url: str, limit: int) -> bytes:
    with _open(url) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise UpdateError("the server sent more data than expected")
    return data


def download(url: str, dest: Path, size: int, sha256: str, progress: Progress | None = None) -> None:
    """Stream into `dest`; the byte count and hash must match the signed manifest exactly or it is deleted."""
    digest, done = hashlib.sha256(), 0
    try:
        with _open(url) as response, dest.open("wb") as output:
            while chunk := response.read(_CHUNK):
                done += len(chunk)
                if done > size:
                    raise UpdateError("the download is larger than the signed manifest says")
                digest.update(chunk)
                output.write(chunk)
                if progress:
                    progress(done, size)
        if done != size or digest.hexdigest() != sha256:
            raise UpdateError("the download does not match its checksum")
    except BaseException:
        dest.unlink(missing_ok=True)
        raise
