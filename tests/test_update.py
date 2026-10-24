import base64
import hashlib
import io
import json
import zipfile
from pathlib import Path

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from maplelens.tray import tray as tray_module
from maplelens.tray.update_panel import describe
from maplelens.update import UpdateError, install, net
from maplelens.update.check import CheckResult, UpdateChecker, download_url, find_update, release_page
from maplelens.update.manifest import Manifest, asset_name, manifest_name, verify_manifest
from maplelens.update.model import Phase, Update, UpdateState
from maplelens.update.version import parse_version

KEY = Ed25519PrivateKey.generate()
PUBLIC = base64.b64encode(KEY.public_key().public_bytes(serialization.Encoding.Raw,
                                                         serialization.PublicFormat.Raw)).decode()


def signed(version="0.2.0", **overrides):
    zip_bytes = b"zip-bytes"
    fields = {"version": version, "asset": asset_name(version), "size": len(zip_bytes),
              "sha256": hashlib.sha256(zip_bytes).hexdigest(), **overrides}
    data = json.dumps(fields).encode()
    return data, base64.b64encode(KEY.sign(data))


def fake_fetch(tag="v0.2.0", manifest_version="0.2.0", prerelease=False, tamper=False):
    data, signature = signed(manifest_version)
    if tamper:
        data = data.replace(b"zip", b"zap")

    def fetch(url, limit):
        if url.endswith("/latest"):
            return json.dumps({"tag_name": tag, "prerelease": prerelease, "draft": False}).encode()
        return signature if url.endswith(".sig") else data
    return fetch


def test_versions_compare_numerically_and_reject_odd_tags():
    assert parse_version("v0.10.0") > parse_version("0.9.9")
    for bad in ("v1.2", "1.2.3-rc1", "latest", None, "v1.2.3\n; x"):
        with pytest.raises(UpdateError):
            parse_version(bad)


def test_valid_signature_gives_a_manifest():
    manifest = verify_manifest(*signed(), PUBLIC)
    assert manifest == Manifest("0.2.0", "maplelens-v0.2.0-windows.zip", 9, hashlib.sha256(b"zip-bytes").hexdigest())


@pytest.mark.parametrize("overrides", [{"asset": "evil.zip"}, {"size": 0}, {"size": "9"}, {"sha256": "ABC"},
                                       {"version": "0.2"}])
def test_signed_but_malformed_manifest_is_refused(overrides):
    with pytest.raises(UpdateError):
        verify_manifest(*signed(**overrides), PUBLIC)


def test_wrong_key_or_tampered_bytes_are_refused():
    data, signature = signed()
    other = base64.b64encode(Ed25519PrivateKey.generate().public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw)).decode()
    with pytest.raises(UpdateError):
        verify_manifest(data, signature, other)
    with pytest.raises(UpdateError):
        verify_manifest(data.replace(b"9", b"8"), signature, PUBLIC)
    with pytest.raises(UpdateError):
        verify_manifest(data, b"not base64!", PUBLIC)


def test_newer_signed_release_is_offered_with_urls_built_locally():
    update = find_update("0.1.3", fake_fetch(), PUBLIC)
    assert update.version == "0.2.0"
    assert update.url == download_url("0.2.0", asset_name("0.2.0"))
    assert update.page_url == release_page("0.2.0")


def test_same_older_and_pre_releases_offer_nothing():
    assert find_update("0.2.0", fake_fetch(), PUBLIC) is None
    assert find_update("0.3.0", fake_fetch(), PUBLIC) is None
    assert find_update("0.1.3", fake_fetch(prerelease=True), PUBLIC) is None


def test_replayed_manifest_of_another_release_is_refused():
    with pytest.raises(UpdateError, match="different version"):
        find_update("0.1.3", fake_fetch(tag="v0.3.0", manifest_version="0.2.0"), PUBLIC)


def test_tampered_manifest_is_refused():
    with pytest.raises(UpdateError):
        find_update("0.1.3", fake_fetch(tamper=True), PUBLIC)


def test_checker_reports_a_failure_without_dying():
    def offline(url, limit):
        raise OSError("offline")

    checker = UpdateChecker("0.1.3", fetch=offline)
    checker.start()
    try:
        assert checker.events.get(timeout=5) == CheckResult(error="could not check")
    finally:
        checker.stop()


def test_checker_stays_quiet_without_auto_check_until_asked():
    checker = UpdateChecker("0.2.0", auto_check=False, fetch=fake_fetch())
    checker.start()
    try:
        assert checker.events.empty()
        checker.check_now()
        assert checker.events.get(timeout=5) == CheckResult(update=None)
    finally:
        checker.stop()


def test_only_https_github_hosts_are_fetched():
    for url in ("http://github.com/x", "https://evil.example/x", "https://github.com.evil.example/x"):
        with pytest.raises(UpdateError):
            net._check_url(url)
    net._check_url("https://objects.githubusercontent.com/x")


class _Response:
    def __init__(self, data):
        self._data = io.BytesIO(data)

    def read(self, n=-1):
        return self._data.read(n)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


def test_download_verifies_size_and_hash_and_deletes_bad_files(tmp_path, monkeypatch):
    payload = b"x" * 1000
    monkeypatch.setattr(net, "_open", lambda url: _Response(payload))
    good = tmp_path / "good.zip"
    net.download("u", good, 1000, hashlib.sha256(payload).hexdigest())
    assert good.read_bytes() == payload
    for size, digest in ((1000, "0" * 64), (999, hashlib.sha256(payload).hexdigest()), (1001, "0" * 64)):
        bad = tmp_path / "bad.zip"
        with pytest.raises(UpdateError):
            net.download("u", bad, size, digest)
        assert not bad.exists()


def make_update(version="0.2.0"):
    return Update(version, Manifest(version, asset_name(version), 1, "0" * 64), "u", "p")


def make_zip(path: Path, members: dict[str, bytes]):
    with zipfile.ZipFile(path, "w") as bundle:
        for name, data in members.items():
            bundle.writestr(name, data)


def test_extract_takes_only_the_expected_exe(tmp_path):
    archive, target = tmp_path / "a.zip", tmp_path / "out.exe"
    make_zip(archive, {"maplelens-v0.2.0-windows/maplelens.exe": b"EXE", "maplelens-v0.2.0-windows/README.md": b"r",
                       "../evil.exe": b"bad"})
    install.extract_exe(archive, make_update(), target)
    assert target.read_bytes() == b"EXE"
    assert not (tmp_path.parent / "evil.exe").exists()


def test_extract_accepts_backslash_names_and_refuses_missing_member(tmp_path):
    archive, target = tmp_path / "a.zip", tmp_path / "out.exe"
    make_zip(archive, {"maplelens-v0.2.0-windows\\maplelens.exe": b"EXE"})
    install.extract_exe(archive, make_update(), target)
    assert target.read_bytes() == b"EXE"
    target.unlink()
    make_zip(archive, {"other/maplelens.exe": b"EXE"})
    with pytest.raises(UpdateError):
        install.extract_exe(archive, make_update(), target)
    assert not target.exists()


def test_swap_moves_the_new_exe_in_and_keeps_the_old_aside(tmp_path):
    exe, staged = tmp_path / "maplelens.exe", tmp_path / install.NEW_NAME
    exe.write_bytes(b"old")
    staged.write_bytes(b"new")
    install.swap(exe, staged)
    assert exe.read_bytes() == b"new"
    assert (tmp_path / install.OLD_NAME).read_bytes() == b"old"


def test_failed_swap_puts_the_old_exe_back(tmp_path):
    exe = tmp_path / "maplelens.exe"
    exe.write_bytes(b"old")
    with pytest.raises(UpdateError):
        install.swap(exe, tmp_path / "missing.exe")
    assert exe.read_bytes() == b"old"


def test_self_test_requires_the_promised_version(tmp_path, monkeypatch):
    class Done:
        returncode, stdout = 0, "maplelens 0.9.9\n"

    monkeypatch.setattr(install.subprocess, "run", lambda *a, **k: Done)
    with pytest.raises(UpdateError):
        install.self_test(tmp_path / "x.exe", "0.2.0")
    install.self_test(tmp_path / "x.exe", "0.9.9")


def test_cleanup_removes_leftovers_only_for_the_packaged_exe(tmp_path, monkeypatch):
    for name in (install.OLD_NAME, install.NEW_NAME):
        (tmp_path / name).write_bytes(b"x")
    install.cleanup_leftovers(tmp_path / "maplelens.exe")
    assert (tmp_path / install.OLD_NAME).exists()   # running from source: nothing is touched
    monkeypatch.setattr(install.sys, "frozen", True, raising=False)
    install.cleanup_leftovers(tmp_path / "maplelens.exe")
    assert not (tmp_path / install.OLD_NAME).exists() and not (tmp_path / install.NEW_NAME).exists()


def test_status_text_and_button_states():
    update = make_update()
    assert describe(UpdateState(Phase.AVAILABLE, update)) == ("Version 0.2.0 is available.", True, True)
    assert describe(UpdateState(Phase.UP_TO_DATE)) == ("You have the latest version.", True, False)
    text, can_check, can_update = describe(UpdateState(Phase.DOWNLOADING, update, "42%"))
    assert "42%" in text and not can_check and not can_update
    assert describe(UpdateState(Phase.FAILED, update, "boom"))[2]   # can retry
    assert not describe(UpdateState(Phase.FAILED, None, "offline"))[2]


def test_tray_tooltip_announces_an_update_and_clears_it():
    tray = tray_module.TrayIcon(lambda: None, lambda: None, lambda: None)
    try:
        assert "update" not in tray.tooltip
        tray.set_update("0.2.0")
        assert tray.tooltip.endswith("new update available (v0.2.0)")
        tray.set_update(None)
        assert "update" not in tray.tooltip
    finally:
        tray.close()


def test_manifest_name_is_stable():
    assert manifest_name("0.2.0") == "maplelens-v0.2.0-manifest.json"
