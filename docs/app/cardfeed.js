// The cards' model: what the page's card panel and the floating window show, fed by the engine's states and the
// player's clicks. Views (cards.js) draw it and tell it where the pointer is. A card is
//   live      while its tooltip (or Character Info window) is being read;
//   leaving   for LINGER_MS once it is gone, the time standing still while the pointer is over a view (the player
//             moved there to read it), then it goes and only the recent items are left (idle);
//   recalled  when the player clicks a recent item, until the next tooltip or Back.

/** How long a card stays after its tooltip is gone (not counting while the pointer is over the view). */
export const LINGER_MS = 6000;
/** No news from the engine for this long (the page was throttled, the share paused) means the tooltip is gone: a frozen
 *  "live" state must not keep a card up. */
const STALE_MS = 1000;
/** Recent items kept, with their cards (one card is ~2 MB of pixels at 1.5x). */
const RECENT_MAX = 6;
const EMPTY = { card: null, rank: null, about: null, title: "" };

/** What the views show, fed by the engine's states and the player's clicks. */
export class CardFeed {
  constructor() {
    this.views = new Set();
    this.mode = "idle";
    this.shown = EMPTY;
    /** [{about, card}], newest first, one per item. */
    this.recent = [];
    this.recentVersion = 0;
    this.sharing = false;
    this.store = null;
    this.holders = new Set();
    /** Cards that started leaving so far: each one's countdown bar starts over. */
    this.leaves = 0;
    this.leaveAt = 0;
    this.left = 0;
    this.timer = 0;
    this.stale = 0;
  }

  /** `card`: the item card's ImageData, `rank`: the rank panel's subject ({read, status, rank}), or null: what the
   *  engine shows now; `about` the item card's {name, price, trend};
   *  `title` the item or player being read; `sharing` whether a game window is being watched; `store` {text, tone}. */
  update({ card, rank, about }, { title = "", sharing = false, store = null } = {}) {
    this.sharing = sharing;
    this.store = store;
    clearTimeout(this.stale);
    if (card || rank) {
      this.stop();
      this.mode = "live";
      this.shown = { card, rank, about: card ? about : null, title: (card && about?.name) || title };
      if (card && about?.name) this.remember(about, card);
      this.stale = setTimeout(() => this.leave(), STALE_MS);
    } else if (this.mode === "live") {
      this.leave();
    }
    this.changed();
  }

  /** The share stopped: nothing is live any more (the recent items stay). */
  clear() {
    clearTimeout(this.stale);
    this.stop();
    if (this.mode !== "recalled") this.go("idle");
  }

  remember(about, card) {
    const top = this.recent[0];
    if (top?.about.name === about.name && top.card === card) return;
    this.recent = [{ about, card }, ...this.recent.filter(r => r.about.name !== about.name)].slice(0, RECENT_MAX);
    this.recentVersion += 1;
  }

  leave() {
    if (this.mode !== "live") return;
    this.mode = "leaving";
    this.leaves += 1;
    this.left = LINGER_MS;
    this.leaveAt = performance.now() + LINGER_MS;
    if (!this.holders.size) this.timer = setTimeout(() => this.go("idle"), LINGER_MS);
    this.changed();
  }

  /** A view's pointer came over it (`on`) or went: a leaving card waits while it is there. */
  hold(view, on) {
    const was = this.holders.size > 0;
    if (on) this.holders.add(view);
    else this.holders.delete(view);
    const held = this.holders.size > 0;
    if (held === was || this.mode !== "leaving") return;
    if (held) {
      clearTimeout(this.timer);
      this.left = Math.max(0, this.leaveAt - performance.now());
    } else {
      this.leaveAt = performance.now() + this.left;
      this.timer = setTimeout(() => this.go("idle"), this.left);
    }
    this.changed();
  }

  /** Show recent item `index` again. */
  recall(index) {
    const entry = this.recent[index];
    if (!entry) return;
    this.stop();
    this.mode = "recalled";
    this.shown = { card: entry.card, rank: null, about: entry.about, title: entry.about.name };
    this.changed();
  }

  go(mode) {
    this.stop();
    this.mode = mode;
    this.shown = EMPTY;
    this.changed();
  }

  stop() {
    clearTimeout(this.timer);
    this.timer = 0;
  }

  get held() {
    return this.holders.size > 0;
  }

  changed() {
    for (const view of this.views) view.render();
  }
}
