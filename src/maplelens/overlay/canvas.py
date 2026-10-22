"""Small drawing toolkit on a transparent RGBA image (the card is composed from these calls)."""

from collections.abc import Callable

from PIL import Image, ImageDraw

from maplelens.overlay import theme
from maplelens.overlay.theme import RGBA

Draw = Callable[[ImageDraw.ImageDraw, int], None]   # (drawer, supersample scale)


class Canvas:
    def __init__(self, width: int, height: int) -> None:
        self.image = Image.new("RGBA", (width, height), (0, 0, 0, 0))
        self.draw = ImageDraw.Draw(self.image)

    @property
    def width(self) -> int:
        return self.image.width

    def text(self, x: float, y: float, text: str, size: int, fill: RGBA = theme.TEXT, *,
             bold: bool = False, anchor: str = "la") -> int:
        """Draw text and return its width in px."""
        font = theme.font(size, bold)
        self.draw.text((x, y), text, font=font, fill=fill, anchor=anchor)
        return round(self.draw.textlength(text, font=font))

    def measure(self, text: str, size: int, bold: bool = False) -> int:
        return round(self.draw.textlength(text, font=theme.font(size, bold)))

    def wrap(self, text: str, size: int, max_width: int, bold: bool = False) -> list[str]:
        """Greedy word wrap."""
        lines, line = [], ""
        for word in text.split():
            trial = f"{line} {word}".strip()
            if line and self.measure(trial, size, bold) > max_width:
                lines.append(line)
                line = word
            else:
                line = trial
        return [*lines, line] if line else lines

    def overlay(self, box: tuple[int, int, int, int], draw: Draw, scale: int = 1) -> None:
        """Run `draw` on a transparent layer for `box` (x, y, w, h) and alpha-blend it in.

        PIL overwrites pixels on RGBA images instead of blending, so every translucent shape goes through
        a layer. `scale` > 1 supersamples the layer, which smooths diagonal lines and round shapes.
        """
        x, y, w, h = box
        layer = Image.new("RGBA", (w * scale, h * scale), (0, 0, 0, 0))
        draw(ImageDraw.Draw(layer), scale)
        if scale > 1:
            layer = layer.resize((w, h), Image.LANCZOS)
        self.image.alpha_composite(layer, (x, y))

    def chip(self, x: int, y: int, text: str, fg: RGBA = theme.TEXT_DIM, *, size: int = 10,
             bold: bool = False, right_align: bool = False, height: int = 16) -> int:
        """Rounded label; returns its width. With `right_align`, x is the chip's right edge."""
        width = self.measure(text, size, bold) + 14
        left = x - width if right_align else x
        self.overlay((left, y, width, height), lambda d, s: d.rounded_rectangle(
            (0, 0, width * s - 1, height * s - 1), radius=5 * s, fill=theme.CHIP_FILL, outline=theme.CHIP_EDGE),
            scale=3)
        self.text(left + 7, y + height / 2, text, size, fg, bold=bold, anchor="lm")
        return width

    def pill(self, x: int, y: int, text: str, colour: RGBA, *, height: int = 20, size: int = 11,
             arrow: bool | None = None) -> int:
        """Solid-tinted status label, e.g. a green "+8.6%". `arrow` True/False adds an up/down triangle."""
        gap = 14 if arrow is not None else 0
        width = self.measure(text, size, True) + 16 + gap
        tint, edge = colour[:3] + (55,), colour[:3] + (150,)
        self.overlay((x, y, width, height), lambda d, s: d.rounded_rectangle(
            (0, 0, width * s - 1, height * s - 1), radius=height // 2 * s, fill=tint, outline=edge), scale=3)
        left = x + 8
        if arrow is not None:
            self.arrow(left, y + height // 2 - 4, arrow, colour, 8)
            left += gap
        self.text(left, y + height / 2, text, size, colour, bold=True, anchor="lm")
        return width

    def arrow(self, x: int, y: int, up: bool, colour: RGBA, size: int = 8) -> None:
        """Filled triangle (Tahoma has no reliable arrow glyphs)."""
        points = [(x, y + size), (x + size, y + size), (x + size / 2, y)] if up else \
                 [(x, y), (x + size, y), (x + size / 2, y + size)]
        self.draw.polygon(points, fill=colour)

    def divider(self, y: int, pad: int) -> None:
        self.draw.line((pad, y, self.width - pad - 1, y), fill=theme.DIVIDER)

    def frame(self, height: int) -> None:
        """The game's tooltip frame: translucent fill, outer line, light inner line."""
        w = self.width
        self.draw.rectangle((0, 0, w - 1, height - 1), fill=theme.PANEL_FILL, outline=theme.EDGE)
        self.draw.rectangle((2, 2, w - 3, height - 3), outline=theme.EDGE_INNER)
