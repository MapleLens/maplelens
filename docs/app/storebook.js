// The store book: what the phone app shows, kept by the page from the shop watcher's events (sales, visitors, chat,
// the store opening and closing) and its status, owner and meso counter. Kept in this browser so a reload keeps the
// history, and uploaded (sealed) to the relay for the paired phones.

const STORAGE_KEY = "maplelens.web.storebook";
/** History kept, newest first: a day of a busy store, and a snapshot well under the relay's size limit. */
const KEEP = { sales: 150, chat: 80, visits: 80 };
/** History older than this is dropped (the phone shows today and yesterday apart, and a week's totals). */
const MAX_AGE_MS = 7 * 24 * 3600 * 1000;

export class StoreBook {
  constructor(storage) {
    this.storage = storage;
    this.book = { owner: null, open: false, openedAt: null, balance: null, status: null, inStore: [], sales: [], chat: [], visits: [] };
    try {
      Object.assign(this.book, JSON.parse(storage(s => s.getItem(STORAGE_KEY)) || "{}"));
    } catch { /* damaged: start over */ }
    // Who was inside when the page closed is not known now.
    this.book.inStore = [];
    this.changed = false;
    /** Item pictures found this session (`setPicture`), by item name. */
    this.pictures = new Map();
  }

  /** The picture of an item of this name: given to its sales without one, and to its later sales. */
  setPicture(name, url) {
    this.pictures.set(name, url);
    let touched = false;
    for (const sale of this.book.sales) {
      if (sale.item === name && !sale.icon) {
        sale.icon = url;
        touched = true;
      }
    }
    if (touched) {
      this.changed = true;
      this.save();
    }
  }

  /** One look's shop state: `{status, owner, balance, events}` as the engine gives it. */
  update(shop, now = Date.now()) {
    if (!shop) return;
    const book = this.book;
    const status = shop.status?.text ? { word: shop.status.status, text: shop.status.text, tone: shop.status.tone } : null;
    if (JSON.stringify(status) !== JSON.stringify(book.status)) this.set("status", status);
    if (shop.owner && shop.owner !== book.owner) this.set("owner", shop.owner);
    if (Number.isFinite(shop.balance) && shop.balance !== book.balance) this.set("balance", shop.balance);
    for (const event of shop.events || []) this.add(event, now);
  }

  add(event, t) {
    const book = this.book;
    switch (event.kind) {
      case "sale":
        book.sales.unshift({ t, item: event.item, qty: event.quantity || 1, buyer: event.buyer, mesos: event.mesos ?? null, icon: this.pictures.get(event.item) || null });
        break;
      case "sale-unseen":
        book.sales.unshift({ t, unseen: true, mesos: event.mesos ?? null });
        break;
      case "entered":
        if (!book.inStore.includes(event.name)) book.inStore.push(event.name);
        book.visits.unshift({ t, name: event.name, in: true });
        break;
      case "left":
        book.inStore = book.inStore.filter(name => name !== event.name);
        book.visits.unshift({ t, name: event.name, in: false });
        break;
      case "chat": {
        // The game's chat lines read "Name : message".
        const at = event.text.indexOf(" : ");
        book.chat.unshift(at > 0 ? { t, name: event.text.slice(0, at), text: event.text.slice(at + 3) } : { t, name: null, text: event.text });
        break;
      }
      case "opened":
        book.open = true;
        book.openedAt = t;
        break;
      case "closed":
        book.open = false;
        book.openedAt = null;
        book.inStore = [];
        break;
      default:
        return;
    }
    this.trim(t);
    this.changed = true;
    this.save();
  }

  set(field, value) {
    this.book[field] = value;
    this.changed = true;
    this.save();
  }

  trim(now) {
    for (const [list, keep] of Object.entries(KEEP)) {
      this.book[list] = this.book[list].filter(entry => now - entry.t < MAX_AGE_MS).slice(0, keep);
    }
  }

  save() {
    this.storage(s => s.setItem(STORAGE_KEY, JSON.stringify(this.book)));
  }

  /** What the phone gets: the book, when it was made, and whether the page is following the store at all. */
  snapshot(following, now = Date.now()) {
    return { v: 1, at: now, following, ...this.book };
  }
}
