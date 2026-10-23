from pathlib import Path

import cv2
import pytest

from maplelens.geometry import Box
from maplelens.overlay.placement import place_beside_window
from maplelens.overlay.rank_card import CARD_WIDTH, render_rank_card
from maplelens.ranking.models import Rank
from maplelens.ranking.service import RankResult, Status
from maplelens.vision import info_window

SHOT = cv2.imread(str(Path(__file__).parent / "fixtures" / "character_info_masterball.png"))
WINDOW = Box(41, 68, 493, 280)   # where the Character Info window sits in that screenshot


def test_finds_the_window_by_its_title():
    x, y = info_window.find_window(cv2.cvtColor(SHOT, cv2.COLOR_BGR2GRAY))
    assert abs(x - WINDOW.x) <= 1 and abs(y - WINDOW.y) <= 1


def test_no_window_where_there_is_none():
    assert info_window.find_window(cv2.cvtColor(SHOT[330:, :], cv2.COLOR_BGR2GRAY)) is None


def test_title_check_follows_the_window():
    area = info_window.title_box(WINDOW)
    crop = cv2.cvtColor(SHOT[area.y : area.bottom, area.x : area.right], cv2.COLOR_BGR2GRAY)
    assert info_window.is_window(crop)
    assert not info_window.is_window(crop[:, ::-1].copy())


def test_name_plate_is_read():
    ocr = pytest.importorskip("maplelens.vision.ocr")
    box = info_window.name_box(WINDOW)
    plate = SHOT[box.y : box.bottom, box.x : box.right]
    text = ocr.TitleReader().read_mask(info_window.name_mask(plate))
    assert info_window.clean_name(text) == "masterball"


def test_clean_name_rejects_ocr_junk():
    assert info_window.clean_name("master ball") == "masterball"
    assert info_window.clean_name("") is None and info_window.clean_name("a-b!") is None


def test_card_sits_left_of_the_window_else_right_else_nowhere():
    bounds = Box(0, 0, 1280, 720)
    assert place_beside_window(Box(400, 100, 493, 280), (230, 238), 4, bounds) == (166, 100)
    assert place_beside_window(Box(41, 68, 493, 280), (230, 238), 4, bounds) == (538, 68)
    assert place_beside_window(Box(41, 68, 1200, 280), (230, 238), 4, bounds) is None
    assert place_beside_window(Box(400, 600, 493, 280), (230, 238), 4, bounds)[1] == 720 - 238


@pytest.mark.parametrize("status", [Status.LOADING, Status.MISSING, Status.FAILED])
def test_card_renders_every_state(status):
    card = render_rank_card(RankResult(status))
    assert card.width == CARD_WIDTH and 70 < card.height < 130


def test_ranked_card_shows_more_than_a_notice():
    rank = Rank("masterball", 27, "Thief", 3, 29516, 824, 21312, False, "2026-10-22T10:16:24Z", None)
    botter = Rank("b", 27, "Thief", 3, 12, 5, 7, True, None, None)
    ranked = render_rank_card(RankResult(Status.FOUND, rank))
    assert ranked.height > render_rank_card(RankResult(Status.MISSING)).height
    assert render_rank_card(RankResult(Status.FOUND, botter)).height < ranked.height


def test_window_is_found_when_the_game_is_drawn_bigger():
    scale = 1.2
    big = cv2.resize(SHOT, None, fx=scale, fy=scale, interpolation=cv2.INTER_CUBIC)
    x, y = info_window.find_window(cv2.cvtColor(big, cv2.COLOR_BGR2GRAY), scale)
    assert abs(x - WINDOW.x * scale) <= 2 and abs(y - WINDOW.y * scale) <= 2
    assert render_rank_card(RankResult(Status.LOADING), ui_scale=scale).width == round(CARD_WIDTH * scale)
