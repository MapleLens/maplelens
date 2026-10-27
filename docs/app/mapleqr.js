// A QR code in MapleLens's colours: navy dots, the three corner finders as navy frames around a maple-orange button,
// the alignment marks as small solid frames, and the meso coin on a white tile in the middle. The same design as the website's code (crates/core/examples/
// phone_qr.rs). Scanners need the finders' dark:light rhythm and enough contrast: the dots are the site's darkest ink
// on white, and the orange centre is dark enough to read as dark; the coin covers `LOGO_SHARE` of the width, which the
// code's high error correction (notify::phone::qr_modules) recovers.

const INK = "#2c3247";
const PAPER = "#ffffff";
const ORANGE_TOP = "#d0700a";
const ORANGE_BOTTOM = "#8a4300";
const EDGE = "#ffad1f";
/** Light modules around the code, as scanners expect. */
const QUIET = 4;
/** notify::phone::LOGO_SHARE. */
const LOGO_SHARE = 0.2;
/** Canvas pixels per module: drawn sharp, shown smaller by CSS. */
const SCALE = 10;
/** A dot's radius in modules: touching neighbours, so a blurred camera image still sees solid runs. */
const DOT = 0.5;
/** How long the code waits for the coin picture before showing without it. */
const COIN_WAIT_MS = 2000;

/** The coin's tile, in modules: odd, so it sits on the centre module. */
export function logoSpan(width) {
  const span = Math.max(7, Math.round(width * LOGO_SHARE));
  return span % 2 ? span : span + 1;
}

/** Whether module (x, y) belongs to one of the three finder patterns (drawn as shapes, not dots). */
function inFinder(x, y, width) {
  const corner = (cx, cy) => x >= cx && x < cx + 7 && y >= cy && y < cy + 7;
  return corner(0, 0) || corner(width - 7, 0) || corner(0, width - 7);
}

/** Alignment pattern centres per version (ISO/IEC 18004, annex E), versions 2 to 15. */
const ALIGNMENT = [[], [], [6, 18], [6, 22], [6, 26], [6, 30], [6, 34], [6, 22, 38], [6, 24, 42], [6, 26, 46], [6, 28, 50],
  [6, 30, 54], [6, 32, 58], [6, 34, 62], [6, 26, 46, 66], [6, 26, 48, 70]];

/** The alignment patterns' centres (5x5 each). Drawn solid: as separate dots, the gaps between them show at large
 *  sizes and break the dark ring a scanner looks for (a version 10 code stopped decoding above ~550 px). */
function alignmentCentres(width) {
  const at = ALIGNMENT[(width - 17) / 4] || [];
  const centres = [];
  for (const y of at) for (const x of at) if (!inFinder(x, y, width)) centres.push([x, y]);
  return centres;
}

function roundRect(g, x, y, w, h, r) {
  g.beginPath();
  g.roundRect(x, y, w, h, r);
}

/** Draws `modules` (width² booleans, row by row) on `canvas`; resolves once the coin is in. */
export async function drawMapleQr(canvas, modules, coinUrl) {
  const width = Math.sqrt(modules.length);
  const size = (width + 2 * QUIET) * SCALE;
  canvas.width = canvas.height = size;
  const g = canvas.getContext("2d");
  const at = n => (n + QUIET) * SCALE;

  g.fillStyle = PAPER;
  roundRect(g, 0, 0, size, size, 3 * SCALE);
  g.fill();
  g.strokeStyle = EDGE;
  g.lineWidth = SCALE * 0.6;
  roundRect(g, g.lineWidth / 2, g.lineWidth / 2, size - g.lineWidth, size - g.lineWidth, 3 * SCALE);
  g.stroke();

  const span = logoSpan(width);
  const from = (width - span) / 2;
  const underLogo = (x, y) => x >= from && x < from + span && y >= from && y < from + span;

  const marks = alignmentCentres(width);
  const inMark = (x, y) => marks.some(([cx, cy]) => Math.abs(x - cx) <= 2 && Math.abs(y - cy) <= 2);

  g.fillStyle = INK;
  for (let y = 0; y < width; y++) {
    for (let x = 0; x < width; x++) {
      if (!modules[y * width + x] || inFinder(x, y, width) || inMark(x, y) || underLogo(x, y)) continue;
      g.beginPath();
      g.arc(at(x) + SCALE / 2, at(y) + SCALE / 2, SCALE * DOT, 0, Math.PI * 2);
      g.fill();
    }
  }

  for (const [cx, cy] of [[0, 0], [width - 7, 0], [0, width - 7]]) {
    // A 7-module navy frame, one module thick, then a light ring and the 3-module orange button.
    g.fillStyle = INK;
    roundRect(g, at(cx), at(cy), 7 * SCALE, 7 * SCALE, 2.2 * SCALE);
    g.fill();
    g.fillStyle = PAPER;
    roundRect(g, at(cx + 1), at(cy + 1), 5 * SCALE, 5 * SCALE, 1.5 * SCALE);
    g.fill();
    const button = g.createLinearGradient(0, at(cy + 2), 0, at(cy + 5));
    button.addColorStop(0, ORANGE_TOP);
    button.addColorStop(1, ORANGE_BOTTOM);
    g.fillStyle = button;
    roundRect(g, at(cx + 2), at(cy + 2), 3 * SCALE, 3 * SCALE, 0.9 * SCALE);
    g.fill();
  }

  for (const [cx, cy] of marks) {
    if (underLogo(cx, cy)) continue;
    g.fillStyle = INK;
    roundRect(g, at(cx - 2), at(cy - 2), 5 * SCALE, 5 * SCALE, 1.4 * SCALE);
    g.fill();
    g.fillStyle = PAPER;
    roundRect(g, at(cx - 1), at(cy - 1), 3 * SCALE, 3 * SCALE, 0.8 * SCALE);
    g.fill();
    g.fillStyle = INK;
    roundRect(g, at(cx), at(cy), SCALE, SCALE, 0.3 * SCALE);
    g.fill();
  }

  // The coin's tile: white, edged in orange, the pixel-art meso drawn sharp.
  const tile = at(from) + SCALE * 0.4;
  const tileSize = span * SCALE - SCALE * 0.8;
  g.fillStyle = PAPER;
  roundRect(g, tile, tile, tileSize, tileSize, 1.6 * SCALE);
  g.fill();
  g.strokeStyle = EDGE;
  g.lineWidth = SCALE * 0.5;
  g.stroke();
  const coin = new Image();
  coin.src = coinUrl;
  // A hidden tab may never finish decoding: the code reads without its coin, so it is not held up for it.
  await Promise.race([coin.decode().catch(() => null), new Promise(done => setTimeout(done, COIN_WAIT_MS))]);
  if (coin.naturalWidth) {
    const inner = tileSize - 2.4 * SCALE;
    const k = Math.floor((inner / Math.max(coin.naturalWidth, coin.naturalHeight)) * 2) / 2;
    const w = coin.naturalWidth * k;
    const h = coin.naturalHeight * k;
    g.imageSmoothingEnabled = false;
    g.drawImage(coin, tile + (tileSize - w) / 2, tile + (tileSize - h) / 2, w, h);
  }
}
