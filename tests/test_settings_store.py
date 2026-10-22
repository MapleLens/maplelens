import json

from maplelens import settings_store as store
from maplelens import startup


def test_missing_file_gives_defaults(tmp_path):
    assert store.load(tmp_path / "none.json") == store.Settings()


def test_roundtrip(tmp_path):
    path = tmp_path / "sub" / "settings.json"
    wanted = store.Settings(launch_at_startup=True, overlay_enabled=False, toggle_hotkey=0, expand_delay_s=0.5)
    store.save(wanted, path)
    assert store.load(path) == wanted


def test_corrupt_file_gives_defaults(tmp_path):
    path = tmp_path / "settings.json"
    path.write_text("{not json", encoding="utf-8")
    assert store.load(path) == store.Settings()


def test_invalid_fields_fall_back_individually_and_unknown_are_ignored(tmp_path):
    path = tmp_path / "settings.json"
    path.write_text(json.dumps({"launch_at_startup": True, "expand_delay_s": 99, "overlay_enabled": "yes",
                                "toggle_hotkey": -1, "future_option": 1}), encoding="utf-8")
    assert store.load(path) == store.Settings(launch_at_startup=True)


def test_startup_registry_roundtrip():
    name = "MapleLensTest"
    try:
        startup.set_enabled(True, name)
        assert startup.is_enabled(name)
        startup.set_enabled(False, name)
        assert not startup.is_enabled(name)
        startup.set_enabled(False, name)   # removing twice is fine
    finally:
        startup.set_enabled(False, name)


def test_launch_command_is_quoted():
    assert startup.launch_command().startswith('"')
