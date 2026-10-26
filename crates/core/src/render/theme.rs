//! Colours and metrics sampled from MapleStory's own item tooltip, so overlays look native.

/// Straight (not premultiplied) RGBA.
pub type Rgba = [u8; 4];

/// The tooltip fill is see-through: measured over dark and bright backdrops it resolves to roughly 80% of
/// (58,62,120). A near-black base at higher alpha keeps the card dark over bright scenes, where the bluer
/// fill washed out to a pale box next to the game's own tooltip.
pub const PANEL_FILL: Rgba = [34, 37, 80, 215];
/// Outer border line.
pub const EDGE: Rgba = [118, 120, 150, 235];
/// Soft inner line.
pub const EDGE_INNER: Rgba = [160, 162, 190, 130];
pub const DIVIDER: Rgba = [200, 204, 235, 60];
/// The dark class pills of equipment tooltips.
pub const CHIP_FILL: Rgba = [36, 34, 62, 150];
pub const CHIP_EDGE: Rgba = [150, 152, 190, 140];
/// Dark groove of bars and charts.
pub const TRACK: Rgba = [20, 22, 60, 150];

/// Blue dot before titles in the game.
pub const BULLET: Rgba = [110, 193, 255, 255];
pub const TEXT: Rgba = [255, 255, 255, 255];
/// Labels: the game prints "REQ LEV" etc. near-white.
pub const TEXT_DIM: Rgba = [226, 228, 246, 255];
/// Like the greyed-out zero requirements (about 105,110,126 on the fill).
pub const TEXT_FAINT: Rgba = [150, 155, 186, 255];
pub const GOLD: Rgba = [255, 211, 77, 255];
pub const GOLD_SOFT: Rgba = [255, 211, 77, 70];
pub const UP: Rgba = [138, 230, 138, 255];
pub const DOWN: Rgba = [255, 140, 140, 255];
pub const WARN: Rgba = [255, 176, 96, 255];
/// "Untradable", usable classes and stat bullets in the game.
pub const ORANGE: Rgba = [255, 158, 46, 255];
pub const VOLUME: Rgba = [120, 170, 255, 120];

/// The game draws its UI about 1.5x bigger than a 1x design at the reference resolution: measured on a native
/// tooltip, body text has a 13 px capital height (Tahoma 12 px at 1x is 8.6) and a 450 px wide tooltip is
/// 300 at 1x. The card is laid out at 1x and drawn at this scale, so it ends up as wide as a tooltip with the
/// same text size.
pub const CARD_SCALE: f32 = 1.5;

/// `colour` with a different alpha.
pub const fn with_alpha(colour: Rgba, alpha: u8) -> Rgba {
    [colour[0], colour[1], colour[2], alpha]
}
