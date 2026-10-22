"""The rank panel shown beside the Character Info window, styled like the game's own windows (pure)."""

from datetime import UTC, datetime

from PIL import Image

from maplelens.market.format import format_ago
from maplelens.overlay import gamewin
from maplelens.overlay.canvas import Canvas
from maplelens.ranking.models import Rank
from maplelens.ranking.service import RankResult, Status

CARD_WIDTH = 204   # native pixels: it sits next to a game window, so no CARD_SCALE
_MAX_HEIGHT = 300
_HERO_H = 30
_NOTE_H = 14   # room under the last row for the 'updated' line
_MESSAGES = {
    Status.LOADING: ("Looking up rank…", "classicrank.com"),
    Status.MISSING: ("Not ranked yet", "No record on classicrank.com"),
    Status.FAILED: ("Rank unavailable", "will retry shortly"),
}


def _number(value: int | None, prefix: str = "") -> str:
    return f"{prefix}{value:,}" if value is not None else "—"


def render_rank_card(result: RankResult, now: datetime | None = None) -> Image.Image:
    """Panel for a player in whatever state their lookup is in (the game window already shows the name)."""
    cv = Canvas(CARD_WIDTH, _MAX_HEIGHT)
    y = gamewin.PANEL_TOP + gamewin.PAD_V
    if result.rank is None:
        y, note = _message(cv, y, *_MESSAGES[result.status]), None
    else:
        y, note = _ranks(cv, y, result.rank, now or datetime.now(UTC))
    if note:
        gamewin.centred(cv, CARD_WIDTH / 2, y + 5, note, 10, gamewin.INK_DIM)
    panel_bottom = y + (_NOTE_H if note else gamewin.PAD_V - gamewin.ROW_GAP)
    height = panel_bottom + 1 + gamewin.STRIP_H
    card = Canvas(CARD_WIDTH, height)
    gamewin.window(card, height, "RANKING")
    card.image.alpha_composite(cv.image.crop((0, gamewin.PANEL_TOP, CARD_WIDTH, height)), (0, gamewin.PANEL_TOP))
    gamewin.button(card, (gamewin.BORDER, panel_bottom + 4, CARD_WIDTH - 2 * gamewin.BORDER, gamewin.BUTTON_H),
                   "VIEW ON CLASSICRANK")
    return card.image


def _ranks(cv: Canvas, y: int, rank: Rank, now: datetime) -> tuple[int, str]:
    label = "BOTTER RANK" if rank.botting else "WORLD RANK"
    y = gamewin.row(cv, y, label, _number(rank.overall_rank, "#"), height=_HERO_H, value_size=17)
    note = f"updated {format_ago(rank.last_checked, now)}"
    if rank.botting:   # the API reuses overall_rank for the Botters list, so job/fame ranks would mislead
        return y, "flagged as botting · " + note
    y = gamewin.row(cv, y, "JOB RANK", _number(rank.job_rank, "#"))
    y = gamewin.row(cv, y, "FAME RANK", _number(rank.fame_rank, "#"))
    return y, note


def _message(cv: Canvas, y: int, headline: str, detail: str) -> int:
    gamewin.value_box(cv, (gamewin.PAD, y, CARD_WIDTH - 2 * gamewin.PAD, 38))
    gamewin.centred(cv, CARD_WIDTH / 2, y + 12, headline, 13, gamewin.INK)
    gamewin.centred(cv, CARD_WIDTH / 2, y + 28, detail, 10, gamewin.INK_DIM)
    return y + 38 + gamewin.ROW_GAP
