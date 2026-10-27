// Cards: how the item card and the rank panel are shown, in the page and in the floating (picture-in-picture)
// window. One view per document; both get the same pictures and the same states:
//   live      a tooltip is being read: the card is bright and follows it;
//   lingering the tooltip is gone: the last card stays, dimmed, for a few seconds (or until the next one if the
//             player "holds" it), so the numbers can still be read after the mouse moves on;
//   empty     nothing to show: a short hint.

import { paint } from "./lens.js?v=f995eb18b58a";

const FIT_KEY = "maplelens.cards.fit";

function storage(action) {
  try { return action(window.localStorage); } catch { return undefined; } // private windows may refuse
}

/** How long the last card stays after its tooltip is gone. */
const LINGER_MS = 3000;
/** No news from the engine for this long (the page was throttled, the share paused) means the tooltip is gone: a frozen
 *  "live" state must not keep a card up. The linger counts from the last news. */
const STALE_MS = 1000;

/** Look shared by the page's panel and the floating window (the page's own sheet styles the rest). */
const CSS = `
.cv{display:flex;flex-direction:column;min-height:0;font:bold 13px Tahoma,Verdana,sans-serif;color:#dbe4ff}
.cv-head{display:flex;align-items:center;gap:8px;padding:7px 10px;background:#101a36;border-bottom:1px solid #2a3a6a;min-height:20px}
.cv-dot{flex:none;width:10px;height:10px;border-radius:50%;background:#59648a;box-shadow:0 0 0 2px #0006;transition:background .2s}
.cv[data-state=live] .cv-dot{background:#4ee08a;box-shadow:0 0 8px #4ee08a99}
.cv[data-state=lingering] .cv-dot{background:#ffb938}
.cv-title{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.cv-hold,.cv-fit{flex:none;font:inherit;font-size:11px;padding:2px 8px;border-radius:10px;border:1px solid #3b4f8c;background:#1a2750;color:#b8c6f5;cursor:pointer}
.cv-hold[aria-pressed=true],.cv-fit[aria-pressed=true]{background:#ffcc33;border-color:#ffcc33;color:#3a2600}
.cv-hold:focus-visible,.cv-fit:focus-visible{outline:2px solid #fff;outline-offset:1px}
.cv-store{display:flex;align-items:center;gap:8px;padding:5px 10px;font-size:12px;background:#0f1833;border-bottom:1px solid #1d2a52;color:#b8c6f5}
.cv-store[hidden]{display:none}
.cv-store i{flex:none;width:9px;height:9px;border-radius:50%;background:#8a94b8}
.cv-store[data-tone=ok] i{background:#4ee08a}.cv-store[data-tone=ok]{color:#bff0d2}
.cv-store[data-tone=warn] i{background:#ffb938}.cv-store[data-tone=warn]{color:#ffd98a;font-weight:bold}
.cv-body{flex:1;min-height:0;display:flex;flex-direction:column;align-items:center;padding:10px;overflow:hidden auto;scrollbar-width:thin;scrollbar-color:#3b4f8c #101a36}
.cv-stack{margin:auto 0;display:flex;flex-direction:column;align-items:center;gap:10px;max-width:100%}
.cv-body canvas{display:block;flex:none;max-width:100%;height:auto;filter:drop-shadow(0 6px 12px #0007);transition:opacity .25s,filter .25s}
.cv[data-fit=true] .cv-body{overflow:hidden}
.cv[data-fit=true] .cv-stack{height:100%;margin:0;justify-content:center}
.cv[data-fit=true] .cv-body canvas{flex:0 1 auto;min-height:0;object-fit:contain}
.cv[data-state=lingering] .cv-body canvas{opacity:.55;filter:grayscale(.5) drop-shadow(0 6px 12px #0007)}
.cv-empty{margin:0;text-align:center;line-height:1.5;color:#9fb0d8;max-width:24em}
.cv-icon{display:block;margin:0 auto 8px;image-rendering:pixelated;opacity:.9}
.cv-empty b{display:block;font-size:15px;color:#eef3ff;margin-bottom:2px}
.cv-foot{padding:4px 10px;font-size:11px;font-weight:normal;color:#8394c4;background:#0e1730;border-top:1px solid #1d2a52;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-height:15px}
.cv[data-compact=true] .cv-head{padding:4px 7px;gap:6px;min-height:16px}
.cv-fit,.cv-hold{display:none}
.cv[data-compact=true] .cv-store{padding:3px 7px;font-size:11px}
.cv[data-compact=true] .cv-body{padding:6px;overflow:hidden auto}
.cv[data-compact=true] .cv-stack{margin:0;gap:6px;width:100%;justify-content:flex-start}
.cv[data-compact=true] .cv-empty{font-size:11px;line-height:1.35}
.cv[data-compact=true] .cv-empty b{font-size:12px}
.cv[data-compact=true] .cv-icon{display:none}
.cv[data-compact=true] .cv-foot{padding:2px 7px;font-size:10px}
@media (prefers-reduced-motion:reduce){.cv-body canvas,.cv-dot{transition:none}}
`;

export class CardView {
  /** @param {Document} doc  the page's or the floating window's document
   *  @param {HTMLElement} parent  where the view goes
   *  @param {{emptyTitle: string, emptyText: string}} text */
  constructor(doc, parent, { emptyTitle, emptyText, icon = "" }) {
    this.doc = doc;
    this.held = false;
    this.shown = { card: null, rank: null, title: "" };
    this.timer = 0;
    this.live = false;
    if (!doc.getElementById("cv-style")) {
      doc.head.append(Object.assign(doc.createElement("style"), { id: "cv-style", textContent: CSS }));
    }
    const el = (tag, className, props = {}) => Object.assign(doc.createElement(tag), { className, ...props });
    this.root = el("section", "cv");
    this.root.dataset.state = "empty";
    this.dot = el("span", "cv-dot");
    this.title = el("span", "cv-title", { textContent: "Waiting for a tooltip" });
    this.hold = el("button", "cv-hold", { type: "button", textContent: "Hold", title: "Keep the last card on screen until the next one" });
    this.hold.setAttribute("aria-pressed", "false");
    this.fit = el("button", "cv-fit", { type: "button", textContent: "Fit", title: "Shrink the cards to fit the window instead of scrolling" });
    this.fit.addEventListener("click", () => this.setFit(this.root.dataset.fit !== "true"));
    this.setFit(storage(s => s.getItem(FIT_KEY)) === "1", false);
    this.hold.addEventListener("click", () => this.setHeld(!this.held));
    const head = el("header", "cv-head");
    head.append(this.dot, this.title, this.fit, this.hold);
    this.empty = el("p", "cv-empty");
    if (icon) this.empty.append(el("img", "cv-icon", { src: icon, alt: "", width: 32, height: 32 }));
    this.empty.append(el("b", "", { textContent: emptyTitle }), emptyText);
    this.onChange = null;
    this.canvases = { card: el("canvas", "cv-card", { hidden: true }), rank: el("canvas", "cv-rank", { hidden: true }) };
    this.body = el("div", "cv-body");
    // The cards keep their own size (a card squeezed into a small window is a card nobody can read) and the body
    // scrolls; `margin: auto` on the stack centres what is short and still lets what is tall scroll from its top.
    const stack = el("div", "cv-stack");
    stack.append(this.empty, this.canvases.card, this.canvases.rank);
    this.body.append(stack);
    this.foot = el("footer", "cv-foot");
    // The Store Permit's status, worded as the Windows app's pill words it ("Press the SOLD ITEMS button").
    this.store = el("div", "cv-store", { hidden: true });
    this.store.append(el("i"), el("span", "cv-store-text"));
    this.root.append(head, this.store, this.body, this.foot);
    parent.append(this.root);
  }

  /** `store` {text, tone} or null (nothing to say). */
  setStore(store) {
    this.store.hidden = !store?.text;
    if (store?.text) {
      this.store.dataset.tone = store.tone || "idle";
      this.store.querySelector(".cv-store-text").textContent = store.text;
    }
  }

  setFit(fit, remember = true) {
    this.root.dataset.fit = String(fit);
    this.fit.setAttribute("aria-pressed", String(fit));
    if (remember) storage(s => s.setItem(FIT_KEY, fit ? "1" : "0"));
  }

  setHeld(held) {
    this.held = held;
    this.hold.setAttribute("aria-pressed", String(held));
    this.hold.textContent = held ? "Held" : "Hold";
    clearTimeout(this.timer);
    this.timer = 0;
    // Letting go starts the countdown for a card that is no longer live.
    if (!held && !this.live && (this.shown.card || this.shown.rank)) this.timer = setTimeout(() => this.clear(), LINGER_MS);
    this.render(this.live);
  }

  /** `images` {card, rank}: ImageData or null, what the engine shows now; `title` the item or player being read;
   *  `note` the small line at the bottom. */
  update({ card, rank }, { title = "", note = "", store = null } = {}) {
    this.setStore(store);
    const live = Boolean(card || rank);
    this.live = live;
    clearTimeout(this.stale);
    if (live) {
      clearTimeout(this.timer);
      this.shown = { card, rank, title };
      this.stale = setTimeout(() => this.goneQuiet(), STALE_MS);
    } else if ((this.shown.card || this.shown.rank) && !this.held) {
      if (!this.timer) this.timer = setTimeout(() => this.clear(), LINGER_MS);
    }
    if (live) this.timer = 0;
    this.foot.textContent = note;
    this.render(live);
  }

  /** The engine stopped reporting while a tooltip was live: treat it as gone since its last report. */
  goneQuiet() {
    this.live = false;
    if (!this.held) this.timer = setTimeout(() => this.clear(), Math.max(0, LINGER_MS - STALE_MS));
    this.render(false);
  }

  clear() {
    this.timer = 0;
    this.shown = { card: null, rank: null, title: "" };
    this.render(false);
  }

  render(live) {
    const { card, rank, title } = this.shown;
    const any = Boolean(card || rank);
    this.root.dataset.state = live ? "live" : any ? "lingering" : "empty";
    for (const which of ["card", "rank"]) {
      const canvas = this.canvases[which];
      const image = this.shown[which];
      if (image && canvas.image !== image) {
        paint(canvas, image);
        canvas.image = image;
      }
      canvas.hidden = !image;
    }
    this.empty.hidden = any;
    this.onChange?.();
    this.title.textContent = live ? title : any ? `${title || "Last card"} · last seen` : "Waiting for a tooltip";
  }
}

/** The floating window's size (CSS px) for a game window of `gameWidth` x `gameHeight`: a narrow strip, a fifth of the
 *  game's width (so it fits the dead space beside the game's HUD at any size), and tall enough for the biggest thing it
 *  shows, an item card with a player's rank panel under it, so it never needs resizing: the page cannot resize it
 *  after opening without a click inside it. */
export function floatingSize(gameWidth, gameHeight, dpr = window.devicePixelRatio || 1) {
  const width = Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, gameWidth * WIDTH_SHARE)));
  const cardWidth = Math.min(width - 2 * PAD, CARD_PX / dpr);
  const height = cardWidth * CARD_ASPECT + GAP + RANK_PX / dpr + 2 * PAD + CHROME;
  return { width, height: Math.round(Math.min(height, gameHeight * MAX_HEIGHT_SHARE)) };
}
const WIDTH_SHARE = 0.2;
const MIN_WIDTH = 300;
const MAX_WIDTH = 440;
/** An item card is 450 px wide at the game's own size and up to this much taller than wide; a rank panel is about 158 px. */
const CARD_PX = 450;
const CARD_ASPECT = 1.4;
const RANK_PX = 158;
const PAD = 6;
const GAP = 6;
/** The header and footer. */
const CHROME = 48;
const MAX_HEIGHT_SHARE = 0.9;

/** The floating window: Chrome's document picture-in-picture, kept above the game. Resolves to null when the browser
 *  has none (or the player refuses). */
export async function openFloating(onClose, { width = MIN_WIDTH, height = 480 } = {}) {
  if (!("documentPictureInPicture" in window)) return null;
  const pip = await documentPictureInPicture.requestWindow({ width: Math.round(width), height: Math.round(height) });
  const doc = pip.document;
  doc.title = "MapleLens";
  doc.head.append(Object.assign(doc.createElement("style"), {
    textContent: `html,body{margin:0;height:100%;background:#16203d;overflow:hidden}body{display:flex}.cv{flex:1}`,
  }));
  const view = new CardView(doc, doc.body, {
    emptyTitle: "Hover an item in the game",
    emptyText: "Its card shows up here for a few seconds after you move on.",
  });
  view.root.dataset.compact = "true";
  doc.addEventListener("keydown", event => {
    if (event.key === "Escape") pip.close();
  });
  pip.addEventListener("pagehide", onClose);
  return { window: pip, view };
}
