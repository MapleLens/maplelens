// Cards: the item card and the rank panel, in the page and in the floating (picture-in-picture) window. Each document
// has a `CardView` of the one `CardFeed` (cardfeed.js), showing one thing at a time: the card while it is live or
// leaving (with a bar running out); else the training card while the player is training; else what to do next and the
// recent items, newest first. So a player grinding sees the EXP card, an item hovered meanwhile takes its place, and
// the EXP card comes back once that card has gone.
// The floating window is one fixed box for the whole session (it sits in a corner of the screen): whatever it shows
// shrinks to fit it, and recent items that do not fit are left out.

import { paint } from "./lens.js?v=349056720992";
import { LINGER_MS } from "./cardfeed.js?v=349056720992";
import { RANK_CSS, rankPanel } from "./rankpanel.js?v=349056720992";

function storage(action) {
  try { return action(window.localStorage); } catch { return undefined; } // private windows may refuse
}

/** The floating window's size, once the player has resized it. (A new key: sizes saved before the window became one
 *  box were for a tall strip of stacked cards.) */
const SIZE_KEY = "maplelens.cards.box";
const COIN = new URL("../assets/meso.png", import.meta.url).href;
/** Under half a percent reads as noise, as on the card. */
const FLAT_TREND = 0.005;
/** Between the item card and the rank panel (px), as the stylesheet has it. */
const STACK_GAP = 8;
/** The rank panel's width (px) when no item card sets it, and the least it is given beside a shrunken card. */
const RANK_WIDTH = 330;
const MIN_RANK_WIDTH = 270;
/** The header's height (px), as the stylesheet has it. */
const HEAD_H = 34;

/** The look shared by the page's panel and the floating window (the page's own sheet styles the rest). */
const CSS = `
.cv{--ink:#eef3ff;--dim:#9fb0d8;--faint:#7183b6;--line:#2a3a6a;--gold:#ffd34d;--up:#8ae68a;--down:#ff8c8c;--live:#4ee08a;--wait:#ffb938;
  position:relative;display:flex;flex-direction:column;min-height:0;min-width:0;font:bold 13px Tahoma,Verdana,sans-serif;color:var(--ink);background:#16203d}
.cv [hidden]{display:none!important}
.cv button{font:inherit;color:inherit;cursor:pointer}
.cv button:focus-visible{outline:2px solid #fff;outline-offset:1px}
.cv-head{flex:none;display:flex;align-items:center;gap:8px;height:${HEAD_H}px;padding:0 10px;background:#101a36;border-bottom:1px solid var(--line);box-sizing:border-box}
.cv-dot{flex:none;width:9px;height:9px;border-radius:50%;background:#59648a;transition:background .2s,box-shadow .2s}
.cv[data-sharing=true] .cv-dot{background:#3c9b6a}
.cv[data-mode=live] .cv-dot{background:var(--live);box-shadow:0 0 0 3px #4ee08a33,0 0 8px #4ee08a99}
.cv[data-mode=leaving] .cv-dot{background:var(--wait);box-shadow:0 0 0 3px #ffb93833}
.cv[data-mode=recalled] .cv-dot{background:#6ec1ff}
.cv-title{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:14px}
.cv[data-mode=idle] .cv-title{font-size:13px;color:var(--dim)}
.cv-brand{flex:none;display:flex;align-items:center;gap:5px;font-size:11px;color:var(--faint);letter-spacing:.02em}
.cv-brand img{width:14px;height:14px;image-rendering:pixelated}
.cv-back{flex:none;display:flex;align-items:center;gap:4px;padding:3px 9px 3px 7px;border-radius:12px;border:1px solid #3b4f8c;background:#1a2750;font-size:11px;color:#c9d5fb}
.cv-back:hover{border-color:var(--gold);color:#fff}
/* Over the body, not above it: the bar coming and going must not resize what is shown. */
.cv-timer{position:absolute;z-index:1;left:0;right:0;top:${HEAD_H}px;height:3px;background:#0c1430;overflow:hidden}
.cv-timer i{display:block;height:100%;background:var(--wait);transform-origin:left;animation:cv-run var(--linger) linear forwards}
.cv[data-held=true] .cv-timer i{animation-play-state:paused;background:#ffd98a}
@keyframes cv-run{from{transform:scaleX(1)}to{transform:scaleX(0)}}
.cv-body{flex:1;min-height:0;display:flex;flex-direction:column;padding:10px;overflow:hidden;box-sizing:border-box}
.cv-stack{flex:none;display:flex;flex-direction:column;align-items:center;gap:${STACK_GAP}px}
.cv-stack > *{flex:none;transition:opacity .25s}
.cv-stack canvas,.cv-training{display:block;filter:drop-shadow(0 6px 12px #0007)}
.cv-training{flex:none;align-self:center}
.cv[data-mode=leaving] .cv-stack > *{opacity:.8}
.cv[data-mode=leaving][data-held=true] .cv-stack > *{opacity:1}
.cv-idle{flex:1;min-height:0;display:flex;flex-direction:column;gap:14px}
.cv-hint{flex:none;display:flex;align-items:center;gap:12px;padding:12px;border-radius:10px;background:#1a2550;border:1px solid var(--line)}
.cv-hint svg{flex:none}
.cv-hint b{display:block;font-size:14px;color:#fff;margin-bottom:2px}
.cv-hint span{display:block;font-size:12px;font-weight:normal;color:var(--dim);line-height:1.35}
.cv-recent{flex:1;min-height:0;display:flex;flex-direction:column;gap:6px}
.cv-recent h3{margin:0;font-size:10px;letter-spacing:.1em;text-transform:uppercase;color:var(--faint)}
.cv-recent ol{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:5px;overflow:hidden;min-height:0}
.cv-row{width:100%;display:flex;align-items:center;gap:8px;height:34px;padding:0 10px;border-radius:7px;border:1px solid var(--line);
  background:#1b2652;text-align:left;box-sizing:border-box;transition:border-color .15s,background .15s}
.cv-row:hover{border-color:#ffcc33aa;background:#22306a}
.cv-row .n{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12.5px}
.cv-row .p{flex:none;display:flex;align-items:center;gap:4px;color:var(--gold);font-size:13px}
.cv-row .p img{width:12px;height:12px;image-rendering:pixelated}
.cv-row .p.none{color:var(--faint);font-size:11px;font-weight:normal}
.cv-row .t{flex:none;min-width:46px;text-align:right;font-size:11px;color:var(--faint)}
.cv-row .t.up{color:var(--up)}.cv-row .t.down{color:var(--down)}
.cv[data-has-recent=false] .cv-hint{margin:auto 0;flex-direction:column;text-align:center;background:none;border:0;padding:8px}
.cv-away{margin:auto;text-align:center;display:grid;gap:10px;justify-items:center;color:var(--dim);font-size:12px;font-weight:normal}
.cv-away b{font-size:14px;color:#fff}
.cv-away button{padding:5px 12px;border-radius:14px;border:1px solid #3b4f8c;background:#1a2750;color:#dbe4ff;font-size:12px;font-weight:bold}
.cv-away button:hover{border-color:var(--gold)}
.cv-store{flex:none;display:flex;align-items:center;gap:8px;padding:6px 10px;font-size:12px;background:#0f1833;border-top:1px solid #1d2a52;color:#b8c6f5}
.cv-store i{flex:none;width:8px;height:8px;border-radius:50%;background:#8a94b8}
.cv-store span{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.cv-store[data-tone=ok] i{background:var(--live)}.cv-store[data-tone=ok]{color:#bff0d2}
.cv-store[data-tone=warn] i{background:var(--wait)}.cv-store[data-tone=warn]{color:#ffd98a}
.cv[data-compact=true] .cv-body{padding:8px;justify-content:center}
.cv[data-compact=false] .cv-brand{display:none}
@media (prefers-reduced-motion:reduce){.cv *{transition:none!important}.cv-timer i{animation:none}}
`;

/** A tooltip with the pointer on it: what "hover an item" means, drawn. */
const HOVER_PICTURE = `<svg width="58" height="44" viewBox="0 0 58 44" aria-hidden="true">
<rect x="10" y="9" width="46" height="33" rx="4" fill="#232a5c" stroke="#8a8fb8" stroke-width="1.5"/>
<circle cx="17" cy="17" r="2.3" fill="#6ec1ff"/><rect x="22" y="15" width="26" height="4" rx="2" fill="#eef3ff"/>
<rect x="16" y="25" width="12" height="11" rx="2" fill="#3a4378"/><rect x="31" y="26" width="19" height="3" rx="1.5" fill="#9fb0d8"/>
<rect x="31" y="32" width="13" height="3" rx="1.5" fill="#9fb0d8"/>
<path d="M3 2 L3 20 L7.5 15.5 L10.5 22 L13.5 20.6 L10.6 14.2 L16.5 14.2 Z" fill="#fff" stroke="#16203d" stroke-width="1.5" stroke-linejoin="round"/>
</svg>`;

export class CardView {
  /** @param {Document} doc  the page's or the floating window's document
   *  @param {HTMLElement} parent  where the view goes
   *  @param {CardFeed} feed  (cardfeed.js)
   *  @param {{compact?: boolean, page?: boolean}} options  compact: the floating window (fits its cards, shows the
   *    brand); page: the hints speak of the page's own Share button */
  constructor(doc, parent, feed, { compact = false, page = false } = {}) {
    this.doc = doc;
    this.feed = feed;
    this.compact = compact;
    this.page = page;
    this.away = null;
    this.painted = { card: null, rank: null, training: null };
    /** Pictures painted so far, and the layout last fitted: refitting reads the layout, so only when it changed. */
    this.paints = 0;
    this.fitted = "";
    this.drawnRecent = -1;
    this.lastName = "";
    if (!doc.getElementById("cv-style")) {
      doc.head.append(Object.assign(doc.createElement("style"), { id: "cv-style", textContent: CSS + RANK_CSS }));
    }
    const el = (tag, className = "", props = {}) => Object.assign(doc.createElement(tag), { className, ...props });
    this.el = el;
    this.root = el("section", "cv");
    this.root.dataset.compact = String(compact);
    this.root.style.setProperty("--linger", `${LINGER_MS}ms`);

    this.title = el("span", "cv-title");
    this.back = el("button", "cv-back", { type: "button", textContent: "✕ Back", title: "Back to the recent items" });
    this.back.addEventListener("click", () => feed.go("idle"));
    const brand = el("span", "cv-brand");
    brand.append(el("img", "", { src: COIN, alt: "" }), "MapleLens");
    this.brand = brand;
    const head = el("header", "cv-head");
    head.append(el("span", "cv-dot"), this.title, this.back, brand);
    this.timerBar = el("div", "cv-timer");

    this.canvas = el("canvas", "cv-card");
    this.rankSlot = el("div", "cv-rank");
    this.stack = el("div", "cv-stack");
    this.stack.append(this.canvas, this.rankSlot);

    this.hintTitle = el("b");
    this.hintText = el("span");
    const words = el("div");
    words.append(this.hintTitle, this.hintText);
    const hint = el("div", "cv-hint");
    hint.innerHTML = HOVER_PICTURE;
    hint.append(words);
    this.list = el("ol");
    this.recentBox = el("section", "cv-recent");
    this.recentBox.append(el("h3", "", { textContent: "Recent · click to see again" }), this.list);
    this.idle = el("div", "cv-idle");
    this.idle.append(hint, this.recentBox);

    this.awayBox = el("div", "cv-away");
    this.awayBack = el("button", "", { type: "button", textContent: "Bring the cards back here" });
    this.awayBack.addEventListener("click", () => this.away?.());
    this.awayBox.append(el("b", "", { textContent: "The cards are floating over the game" }),
      "Close the floating window to see them here again.", this.awayBack);

    this.training = el("canvas", "cv-training");
    this.body = el("div", "cv-body");
    this.body.append(this.training, this.stack, this.idle, this.awayBox);
    // Not a <footer>: the site's sheet spaces those out (the page's view is in its document).
    this.store = el("div", "cv-store");
    this.storeText = el("span");
    this.store.append(el("i"), this.storeText);
    this.root.append(head, this.timerBar, this.body, this.store);
    this.root.addEventListener("pointerenter", () => feed.hold(this, true));
    this.root.addEventListener("pointerleave", () => feed.hold(this, false));
    parent.append(this.root);

    const Observer = doc.defaultView?.ResizeObserver || ResizeObserver;
    new Observer(() => this.fit()).observe(this.body);
    feed.views.add(this);
    this.render();
  }

  /** The page's view while the cards float: `back` brings them back; null shows the cards again. */
  setAway(back) {
    this.away = back;
    this.render();
  }

  detach() {
    this.feed.views.delete(this);
    this.feed.hold(this, false);
  }

  render() {
    const feed = this.feed;
    const { mode, shown } = feed;
    const away = Boolean(this.away);
    const showing = !away && mode !== "idle";
    const set = (key, value) => { if (this.root.dataset[key] !== value) this.root.dataset[key] = value; };
    set("mode", mode);
    set("sharing", String(feed.sharing));
    set("held", String(feed.held));
    set("hasRecent", String(feed.recent.length > 0));

    // One thing at a time: what the player is looking at, else the training while there is some, else the idle view.
    const training = away || showing ? null : feed.training;
    const title = showing ? shown.title || (shown.rank ? "Character Info" : "Item")
      : training ? feed.trainingTitle || "Training"
        : feed.sharing ? "Watching the game" : "Not watching the game";
    if (this.title.textContent !== title) this.title.textContent = title;
    this.back.hidden = mode !== "recalled" || away;
    this.brand.hidden = !this.compact || showing;

    // The bar starts over for every card that starts leaving.
    const leaving = !away && mode === "leaving";
    if (leaving && this.leaving !== feed.leaves) {
      this.leaving = feed.leaves;
      this.root.style.setProperty("--linger", `${feed.linger}ms`);
      this.timerBar.replaceChildren(this.el("i"));
    }
    if (!leaving) this.leaving = 0;
    this.timerBar.hidden = !leaving;

    this.stack.hidden = !showing;
    this.idle.hidden = away || showing || Boolean(training);
    this.awayBox.hidden = !away;
    if (showing) this.paint(shown);
    else this.lastName = "";
    if (training && this.painted.training !== training) {
      paint(this.training, training);
      this.painted.training = training;
      this.paints += 1;
    }
    this.training.hidden = !training;
    if (!this.idle.hidden) this.renderIdle();

    const store = feed.store;
    this.store.hidden = !store?.text;
    if (store?.text) {
      if (this.store.dataset.tone !== (store.tone || "idle")) this.store.dataset.tone = store.tone || "idle";
      if (this.storeText.textContent !== store.text) this.storeText.textContent = store.text;
    }
    if (this.layoutKey() !== this.fitted) this.fit();
  }

  paint(shown) {
    if (this.painted.card !== shown.card) {
      if (shown.card) paint(this.canvas, shown.card);
      this.painted.card = shown.card;
      this.paints += 1;
    }
    if (this.painted.rank !== shown.rank) {
      this.rankSlot.replaceChildren(...(shown.rank ? [rankPanel(this.doc, shown.rank)] : []));
      this.painted.rank = shown.rank;
      this.paints += 1;
    }
    this.canvas.hidden = !shown.card;
    // One thing at a time here too: an item hovered with a Character Info window open is what the player looks at.
    this.rankSlot.hidden = !shown.rank || Boolean(shown.card);
    const name = shown.title;
    if (name !== this.lastName) {
      // A new item comes in with a short rise, so the eye catches that the card changed.
      if (this.lastName && !this.doc.defaultView?.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
        this.stack.animate?.([{ opacity: 0.2, transform: "translateY(6px)" }, { opacity: 1, transform: "none" }], { duration: 180, easing: "ease-out" });
      }
      this.lastName = name;
    }
  }

  renderIdle() {
    const { sharing, recent, recentVersion } = this.feed;
    const [title, text] = sharing
      ? ["Hover an item in the game", "Its price shows up here."]
      : this.page
        ? ["Share your game window", "Press Share game window, then hover an item in the game."]
        : ["Share your game window", "On the MapleLens page, press Share game window."];
    if (this.hintTitle.textContent !== title) this.hintTitle.textContent = title;
    if (this.hintText.textContent !== text) this.hintText.textContent = text;
    this.recentBox.hidden = !recent.length;
    if (this.drawnRecent === recentVersion) return;
    this.drawnRecent = recentVersion;
    const el = this.el;
    this.list.replaceChildren(...recent.map(({ about }, index) => {
      const row = el("button", "cv-row", { type: "button", title: `Show the ${about.name} card again` });
      row.addEventListener("click", () => this.feed.recall(index));
      const price = el("span", about.price ? "p" : "p none");
      if (about.price) price.append(about.price, el("img", "", { src: COIN, alt: "mesos" }));
      else price.textContent = "no price data";
      const trend = el("span", "t");
      const t = about.trend;
      if (about.price && typeof t === "number") {
        const flat = Math.abs(t) < FLAT_TREND;
        trend.className = `t ${flat ? "" : t > 0 ? "up" : "down"}`;
        trend.textContent = flat ? "steady" : `${t > 0 ? "▲" : "▼"} ${(Math.abs(t) * 100).toFixed(1)}%`;
      }
      row.append(el("span", "n", { textContent: about.name }), price, trend);
      const item = el("li");
      item.append(row);
      return item;
    }));
  }

  /** What the last fit depends on besides the view's size (the observer refits on that). */
  layoutKey() {
    return `${this.feed.mode}|${Boolean(this.away)}|${this.paints}|${this.feed.recentVersion}|${this.store.hidden}|${this.training.hidden}`;
  }

  /** The card shown (the training card, or the item card) at its own size, or smaller to fit (the floating window: its
   *  whole height too; the page: its width), never larger, which would blur it; the rank panel as wide as the card
   *  and its own height (its text does not wrap). Recent items that do not fit are left out rather than scrolled to. */
  fit() {
    this.fitted = this.layoutKey();
    const body = this.body;
    const style = this.doc.defaultView.getComputedStyle(body);
    const width = body.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const height = body.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    if (width <= 0 || (this.compact && height <= 0)) return; // not laid out yet: the observer calls again
    const dpr = window.devicePixelRatio || 1;
    if (!this.training.hidden && this.training.width) {
      const [w, h] = [this.training.width / dpr, this.training.height / dpr];
      const k = Math.max(0, Math.min(1, width / w, this.compact ? height / h : 1));
      this.training.style.width = `${Math.floor(w * k)}px`;
      this.training.style.height = `${Math.floor(h * k)}px`;
    }
    if (!this.stack.hidden) {
      const card = !this.canvas.hidden && this.canvas.width ? this.canvas : null;
      const rank = this.rankSlot.hidden ? null : this.rankSlot;
      const [w, h] = card ? [card.width / dpr, card.height / dpr] : [0, 0];
      let shownWidth = Math.min(width, RANK_WIDTH);
      if (rank) rank.style.width = `${Math.min(width, w || RANK_WIDTH)}px`;
      if (card) {
        const below = rank ? rank.offsetHeight + STACK_GAP : 0;
        const k = Math.max(0, Math.min(1, width / w, this.compact ? (height - below) / h : 1));
        shownWidth = Math.floor(w * k);
        card.style.width = `${shownWidth}px`;
        card.style.height = `${Math.floor(h * k)}px`;
      }
      if (rank) rank.style.width = `${Math.max(Math.min(width, MIN_RANK_WIDTH), shownWidth)}px`;
    }
    if (this.compact && !this.idle.hidden) {
      for (const item of this.list.children) item.hidden = false;
      const bottom = this.list.getBoundingClientRect().bottom;
      const cut = [...this.list.children].filter(item => item.getBoundingClientRect().bottom > bottom + 0.5);
      for (const item of cut) item.hidden = true;
    }
  }
}

/** The floating window's size (CSS px): one box, the same whatever it shows, in proportion to the screen (the game's
 *  UI, and so its cards, grow with the screen's height). Its inside is square: the training card fits it at about its
 *  own size on a 1200 px screen, and a price card shrinks to its width. A size the player gave it by dragging wins. */
export function floatingSize() {
  const saved = storage(s => JSON.parse(s.getItem(SIZE_KEY) || "null"));
  if (Array.isArray(saved) && saved.every(n => Number.isFinite(n) && n >= 200)) {
    return { width: Math.min(saved[0], screen.availWidth), height: Math.min(saved[1], screen.availHeight) };
  }
  const side = Math.round(Math.min(MAX_SIDE, Math.max(MIN_SIDE, (screen.height || 1080) * SIDE_SHARE)));
  return { width: side, height: side + HEAD_H };
}
const SIDE_SHARE = 0.28;
const MIN_SIDE = 280;
const MAX_SIDE = 480;

/** The floating window: Chrome's document picture-in-picture, kept above the game. Resolves to null when the browser
 *  has none (or the player refuses). */
export async function openFloating(feed, onClose, { width, height }) {
  if (!("documentPictureInPicture" in window)) return null;
  const pip = await documentPictureInPicture.requestWindow({ width: Math.round(width), height: Math.round(height) });
  const doc = pip.document;
  doc.title = "MapleLens";
  doc.head.append(Object.assign(doc.createElement("style"), {
    textContent: `html,body{margin:0;height:100%;background:#16203d;overflow:hidden}body{display:flex}.cv{flex:1}`,
  }));
  const view = new CardView(doc, doc.body, feed, { compact: true });
  doc.addEventListener("keydown", event => {
    if (event.key === "Escape") pip.close();
  });
  // Only a size the player dragged it to: the window reports its first size as a resize too.
  let saving = 0;
  pip.addEventListener("resize", () => {
    clearTimeout(saving);
    saving = setTimeout(() => {
      const size = [pip.innerWidth, pip.innerHeight];
      if (Math.abs(size[0] - width) > 2 || Math.abs(size[1] - height) > 2) storage(s => s.setItem(SIZE_KEY, JSON.stringify(size)));
    }, 500);
  });
  pip.addEventListener("pagehide", () => {
    view.detach();
    onClose();
  });
  return { window: pip, view };
}
