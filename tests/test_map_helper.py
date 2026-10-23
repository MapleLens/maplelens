from pathlib import Path

import cv2
import numpy as np
import pytest
from PIL import Image

from maplelens.geometry import Box
from maplelens.mapinfo import resolve
from maplelens.mapinfo.client import MapDataError
from maplelens.mapinfo.icons import IconCache
from maplelens.mapinfo.models import Location, MapInfo, Mob, Npc
from maplelens.mapinfo.service import MapService, Status
from maplelens.mapinfo.vote import Voter
from maplelens.overlay.map_card import SPINNER_FRAMES, render_loading_card, render_map_card
from maplelens.vision import map_tooltip
from maplelens.vision.ocr import TextLine

FIXTURES = Path(__file__).parent / "fixtures"
SEARCH = [{"name": "Henesys", "streetName": "Victoria Road", "id": 100000000},
          {"name": "Henesys Market", "streetName": "Victoria Road", "id": 100000100},
          {"name": "Henesys Hunting Ground I", "streetName": "Victoria Road", "id": 104040000},
          {"name": "Henesys Hunting Ground II", "streetName": "Victoria Road", "id": 104040001}]
INFO = MapInfo(104040000, "Henesys Hunting Ground I", "Victoria Road", False,
               (Mob(100101, "Blue Snail", 2, 4, 15, 9), Mob(130101, "Red Snail", 4, 8, 45, 3)),
               (Npc(1, "Anne", "Guide"),))


def _png(size=(20, 20)) -> bytes:
    import io
    out = io.BytesIO()
    Image.new("RGBA", size, (200, 80, 80, 255)).save(out, "PNG")
    return out.getvalue()


def test_parse_location_reads_street_and_name():
    assert resolve.parse_location("Victoria Road : Henesys") == Location("Victoria Road", "Henesys")
    assert resolve.parse_location("Victoria Road ; Henesys Hunting Ground I").name == "Henesys Hunting Ground I"


def test_parse_location_without_colon_is_a_bare_name():
    assert resolve.parse_location("Victoria Road Henesys Hunting Ground j") == \
        Location("", "Victoria Road Henesys Hunting Ground j")


@pytest.mark.parametrize("text", ["", "Snail", "double-click to send whispers", "12:30 : 45", "Cure: 12"])
def test_parse_location_rejects_other_text(text):
    assert resolve.parse_location(text) is None


def test_pick_matches_name_and_street_despite_ocr_slips():
    assert resolve.pick(Location("Victoria Road", "Henesys Hunting Ground l"), SEARCH) == 104040000
    assert resolve.pick(Location("Victoria Road", "Henesys"), SEARCH) == 100000000
    assert resolve.pick(Location("", "Victoria Road Henesys Hunting Ground j"), SEARCH) == 104040000
    assert resolve.pick(Location("Victoria Road", "Kerning City Subway"), SEARCH) is None


def test_identical_titles_with_look_alike_glyphs_share_a_key():
    assert resolve.key(Location("Victoria Road", "Ground l")) == resolve.key(Location("Victoria Road", "Ground I"))


def _line(text, x, y, w=200, h=16):
    return TextLine(text, x, y, w, h)


def test_find_tooltip_builds_the_box_around_title_and_description():
    lines = [_line("Victoria Road : Henesys", 60, 20), _line("A bowman town located in a wide prairie", 10, 48, 300),
             _line("where you can choose to become a", 10, 70, 280), _line("Florina Beach", 400, 120)]
    found = map_tooltip.find_tooltip(lines)
    assert found.location == Location("Victoria Road", "Henesys")
    assert found.box.x < 10 and found.box.right > 310
    assert found.box.y < 20 and found.box.bottom >= 86   # the far-away label is not part of the tooltip
    assert found.box.bottom < 120


def test_find_tooltip_joins_a_title_split_into_two_lines():
    found = map_tooltip.find_tooltip([_line("Henesys Hunting Ground I", 200, 130), _line("Victoria Road", 100, 131)])
    assert found.location.name == "Victoria Road Henesys Hunting Ground I"


def test_find_tooltip_ignores_noise():
    assert map_tooltip.find_tooltip([_line("x", 0, 0, 5), _line("Buy 10 for 5,000", 0, 30)]) is None


def test_scene_without_a_tooltip_is_not_scanned():
    scene = cv2.imread(str(FIXTURES / "map_tooltip_town.png"))[0:150, 0:300]
    assert not map_tooltip.may_hold_tooltip(scene, map_tooltip.text_mask(scene))


@pytest.mark.parametrize(("name", "crop", "title", "street"), [
    ("map_tooltip_town.png", (150, 150, 400, 150), "Henesys", "Victoria Road"),
    ("map_tooltip_hunting_ground.png", (100, 105, 500, 60), "Hunting Ground", ""),
])
def test_reads_the_sample_tooltips(name, crop, title, street):
    from maplelens.vision.ocr import TitleReader
    x, y, w, h = crop
    image = cv2.imread(str(FIXTURES / name))[y : y + h, x : x + w]
    found = map_tooltip.find_tooltip(TitleReader().read_lines(map_tooltip.text_mask(image)))
    assert found is not None and title in found.location.name and found.location.street == street
    assert found.box.w > 300


def test_icons_download_once_and_survive_restarts(tmp_path):
    calls = []

    def fetch(kind, entity_id):
        calls.append((kind, entity_id))
        return _png()

    cache = IconCache(tmp_path, fetch)
    assert cache.get("mob", 7) is None
    assert cache.download("mob", 7) and cache.download("mob", 7)
    assert calls == [("mob", 7)]
    assert cache.get("mob", 7).size == (20, 20)
    assert (tmp_path / "mob" / "7.png").exists()
    assert IconCache(tmp_path, fetch).get("mob", 7) is not None   # a new run reads the folder, not the network


def test_failed_icon_download_leaves_nothing_behind(tmp_path):
    def fetch(kind, entity_id):
        raise MapDataError("offline")

    cache = IconCache(tmp_path, fetch)
    assert not cache.download("npc", 1)
    assert not cache.has("npc", 1)


def _service(tmp_path, find, fetch, clock=lambda: 0.0):
    return MapService(IconCache(tmp_path, lambda kind, i: _png()), find=find, fetch=fetch, clock=clock)


def test_service_finds_the_map_and_downloads_sprites(tmp_path):
    service = _service(tmp_path, lambda loc: 1, lambda map_id: INFO)
    here = Location("Victoria Road", "Henesys Hunting Ground I")
    assert service.get(here).status is Status.LOADING   # queued, answered by the worker
    service.refresh(here)
    result = service.get(here)
    assert (result.status, result.info, result.icons) == (Status.FOUND, INFO, 3)
    assert service.icons.get("mob", 100101) is not None


def test_service_remembers_unknown_names_and_retries_failures_later(tmp_path):
    now = [0.0]

    def find(loc):
        if loc.name == "Nowhere":
            return None
        raise MapDataError("offline")

    service = _service(tmp_path, find, lambda map_id: INFO, lambda: now[0])
    nowhere, flaky = Location("", "Nowhere"), Location("", "Flaky Place")
    service.refresh(nowhere)
    service.refresh(flaky)
    assert service.get(nowhere).status is Status.MISSING
    assert service.get(flaky).status is Status.FAILED
    assert service._queue.empty()   # neither is looked up again right away
    now[0] = 1e9
    service.get(flaky)
    assert service._queue.qsize() == 1


def test_card_renders_with_and_without_sprites():
    sprite = Image.new("RGBA", (60, 50), (10, 200, 10, 255))
    with_sprites = render_map_card(INFO, lambda kind, i: sprite)
    without = render_map_card(INFO, lambda kind, i: None)
    assert with_sprites.size == without.size and with_sprites.tobytes() != without.tobytes()
    assert render_map_card(INFO, lambda kind, i: None, ui_scale=2.0).width == 2 * with_sprites.width
    assert Box(0, 0, *with_sprites.size).h < 700 * 1.5


def test_card_for_a_town_and_an_empty_map():
    town = MapInfo(1, "Henesys", "Victoria Road", True, (), tuple(Npc(i, f"Npc {i}", "Role") for i in range(20)))
    assert render_map_card(town, lambda kind, i: None).height > 300
    empty = MapInfo(2, "Quiet Place", "Road", False, (), ())
    assert render_map_card(empty, lambda kind, i: None).height < render_map_card(town, lambda k, i: None).height


def test_pipeline_scan_reports_the_tooltip_relative_to_the_cursor(monkeypatch):
    from maplelens import map_pipeline
    from maplelens.vision.ocr import TitleReader
    sample = cv2.imread(str(FIXTURES / "map_tooltip_town.png"))
    screen = np.zeros((1200, 1920, 3), np.uint8)   # the measured resolution, so the scan runs unscaled
    screen[: sample.shape[0], : sample.shape[1]] = sample
    cursor = (127, 128)   # fingertip of the hand in the sample

    class Grabber:
        def grab(self, area):
            return screen[area.y : area.y + area.h, area.x : area.x + area.w]

    monkeypatch.setattr(map_pipeline.game_window, "cursor_pos", lambda: cursor)
    pipeline = map_pipeline.MapPipeline()
    bounds = Box(0, 0, 1920, 1200)
    reader = TitleReader()
    first = pipeline._scan(Grabber(), reader, bounds)
    pipeline._scan(Grabber(), reader, bounds)
    hover = pipeline._scan(Grabber(), reader, bounds)
    assert first is None   # a reading must repeat before a card may show
    assert hover.location == Location("Victoria Road", "Henesys")
    # right of and below the cursor; OCR noise in the blurry sample widens the box to the left
    assert 5 < hover.offset[0] < 45 and 20 < hover.offset[1] < 50
    assert hover.size[0] > 300


def test_voter_settles_noisy_readings_and_holds_the_answer():
    voter = Voter()
    assert voter.add("victoriaroadhenesyshuntinggroundi", "A") is None   # one reading is not yet agreement
    assert voter.add("victoriaroadhenesyshuntinggroundii", "B") is None
    held = voter.add("victoriaroadhenesyshuntinggroundi", "C")
    assert held in ("A", "B", "C")
    assert voter.add("cashshopmenu", "X") == held                         # a stray reading changes nothing
    assert voter.add("victoriaroadhenesyshuntinggroundi", "D") == held


def test_voter_switches_to_a_new_tooltip_once_it_repeats():
    voter = Voter()
    for _ in range(3):
        voter.add("henesyshuntinggroundi", "old")
    answers = [voter.add("theforesteastofhenesys", "new") for _ in range(6)]
    assert answers[-1] == "new"


def test_parse_location_drops_ocr_junk_around_the_title():
    assert resolve.parse_location("Victoria Road : Henesys Hunting Ground I l/").name == "Henesys Hunting Ground I"
    assert resolve.parse_location("i Warning Street : The Road to the Dungeon").street == "Warning Street"


def test_loading_card_animates_and_failure_notice_differs():
    frames = {render_loading_card("Henesys", i).tobytes() for i in range(SPINNER_FRAMES)}
    assert len(frames) == SPINNER_FRAMES
    assert render_loading_card("Henesys", 0, failed=True).tobytes() != render_loading_card("Henesys", 0).tobytes()


def test_store_keeps_maps_and_title_readings_between_runs(tmp_path):
    from maplelens.mapinfo.store import MapStore
    store = MapStore(tmp_path)
    here = Location("Victoria Road", "Henesys Hunting Ground I")
    assert store.find(here) is None and store.load(104040000) is None
    store.remember(here, 104040000)
    store.save(INFO)
    again = MapStore(tmp_path)   # a new run
    assert again.find(Location("Victoria Road", "Henesys Hunting Ground l")) == 104040000   # look-alike glyphs
    assert again.load(104040000) == INFO


def test_service_answers_from_the_disk_cache_without_the_network(tmp_path):
    from maplelens.mapinfo.store import MapStore
    calls = []

    def find(loc):
        calls.append("find")
        return 104040000

    def fetch(map_id):
        calls.append("fetch")
        return INFO

    here = Location("Victoria Road", "Henesys Hunting Ground I")
    for _ in range(2):   # the second service is a restart: it must not need find or fetch
        service = MapService(IconCache(tmp_path / "icons", lambda kind, i: _png()), MapStore(tmp_path / "maps"),
                             find, fetch)
        service.refresh(here)
        assert service.get(here).info == INFO
    assert calls == ["find", "fetch"]


def test_card_lists_everything_and_adds_columns_only_when_the_window_is_too_short():
    town = MapInfo(1, "Henesys", "Victoria Road", True, (), tuple(Npc(i, f"Npc {i}", "Role") for i in range(25)))
    tall = render_map_card(town, lambda kind, i: None, max_size=(2000, 3000))
    # nothing is cut off: the same 25 NPCs just flow into more columns when the window is short
    squeezed = render_map_card(town, lambda kind, i: None, max_size=(2000, 800))
    assert squeezed.height <= 800 and tall.height > 800
    assert squeezed.width > tall.width
    narrow = render_map_card(town, lambda kind, i: None, max_size=(300, 800))
    assert narrow.width == tall.width   # never wider than allowed: a single column is all that fits


def test_hyphenated_names_survive_and_symbols_for_the_colon_are_dropped():
    assert resolve.parse_location("Victoria Road : 3-Way Road-Split").name == "3-Way Road-Split"
    assert resolve.parse_location("Victoria Road � 3-VVay Road-Split .").name == "Victoria Road 3-VVay Road-Split"


def test_numbered_neighbours_do_not_confuse_the_match():
    assert resolve.pick(Location("Victoria Road", "Henesys Hunting Ground I"), SEARCH) == 104040000
    assert resolve.pick(Location("Victoria Road", "Henesys Hunting Ground II"), SEARCH) == 104040001
    near_tie = [{"name": "Ground A", "streetName": "S", "id": 1}, {"name": "Ground B", "streetName": "S", "id": 2}]
    assert resolve.pick(Location("S", "Ground C"), near_tie) is None   # two equally likely maps: no guess


def test_a_rejected_reading_is_ignored_so_another_can_win(monkeypatch):
    from maplelens import map_pipeline
    pipeline = map_pipeline.MapPipeline()
    here = Location("Victoria Road", "Henesys Hunting Gr0und I")
    for _ in range(3):
        pipeline._voter.add(resolve.key(here), (here, (0, 0), (10, 10)))
    pipeline.reject(here)
    assert pipeline._voter.add("other", (here, (0, 0), (10, 10))) is None   # voting restarts from scratch
    assert pipeline._rejected[resolve.key(here)] > 0


def test_map_scan_stands_down_while_an_item_tooltip_is_showing(monkeypatch):
    from maplelens import map_pipeline
    pipeline = map_pipeline.MapPipeline()
    pipeline._voter.add("reading", "payload")
    pipeline.item_showing.set()
    boom = lambda *a: (_ for _ in ()).throw(AssertionError("must not grab the screen"))   # noqa: E731
    monkeypatch.setattr(map_pipeline.game_window, "cursor_pos", boom)
    assert pipeline._scan(None, None, Box(0, 0, 1920, 1200)) is None
    assert pipeline._voter._recent == type(pipeline._voter._recent)()   # old readings are forgotten
