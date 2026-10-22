"""Rebuild assets/maplelens.ico from the meso coin sprite: `python scripts/make_icon.py`.

The sprite is 23x24 pixel art, so enlarging uses nearest-neighbour to keep the pixels crisp; only the 16 px
size (smaller than the sprite) is downscaled smoothly.
"""

from pathlib import Path

from PIL import Image

ASSETS = Path(__file__).resolve().parents[1] / "src" / "maplelens" / "assets"
SIZES = (16, 24, 32, 48, 64, 128, 256)


def main() -> None:
    coin = Image.open(ASSETS / "meso.png").convert("RGBA")
    side = max(coin.size)
    square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    square.paste(coin, ((side - coin.width) // 2, (side - coin.height) // 2))
    frames = [square.resize((s, s), Image.Resampling.LANCZOS if s < side else Image.Resampling.NEAREST)
              for s in SIZES]
    frames[-1].save(ASSETS / "maplelens.ico", sizes=[(s, s) for s in SIZES], append_images=frames[:-1])


if __name__ == "__main__":
    main()
