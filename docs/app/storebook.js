// The store book: what the phone app shows, kept by the page from the shop watcher's events (sales, visitors, chat,
// the store opening and closing) and its status, owner and meso counter. Kept in this browser so a reload keeps the
// history, and uploaded (sealed) to the relay for the paired phones.

const STORAGE_KEY = "maplelens.web.storebook";
/** History kept, newest first: a day of a busy store, and a snapshot well under the relay's size limit. */
const KEEP = { sales: 150, chat: 80, visits: 80 };
/** Past stores kept: a week of a player who opens a few a day. */
const KEEP_SESSIONS = 30;
/** History older than this is dropped (the phone shows today and yesterday apart, and a week's totals). */
const MAX_AGE_MS = 7 * 24 * 3600 * 1000;
/** A store seen opening while the book still has one open continues it when that one was busy this recently: the
 *  page reloaded over the same store, whose watcher reports it opening again. */
const RESUME_WITHIN_MS = 30 * 60 * 1000;

export class StoreBook {
  constructor(storage) {
    this.storage = storage;
    this.book = { owner: null, open: false, openedAt: null, balance: null, status: null, inStore: [], sales: [], chat: [], visits: [], sessions: [] };
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

  /** The items whose sales have no picture, each once. */
  unpictured() {
    return [...new Set(this.book.sales.filter(sale => sale.item && !sale.icon).map(sale => sale.item))];
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
        // A sale of an item with the game's id comes with its picture; others get one once found by name.
        if (event.picture) this.pictures.set(event.item, event.picture);
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
      case "opened": {
        const last = this.lastActivity();
        if (book.open && book.openedAt && last !== null && t - last <= RESUME_WITHIN_MS) break;
        // One the book still has open was not seen closing (the page was closed): it ended when last busy.
        if (book.open) this.endSession(last ?? t);
        book.open = true;
        book.openedAt = t;
        break;
      }
      case "closed":
        this.endSession(t);
        book.open = false;
        book.inStore = [];
        break;
      default:
        return;
    }
    this.trim(t);
    this.changed = true;
    this.save();
  }

  /** The newest moment the open store was known busy: its opening or its latest entry. */
  lastActivity() {
    const book = this.book;
    const times = [book.openedAt, book.sales[0]?.t, book.chat[0]?.t, book.visits[0]?.t].filter(Number.isFinite);
    return times.length ? Math.max(...times) : null;
  }

  /** The open store, ended at `to`, into the past stores. Its numbers are counted from the history kept, so they are
   *  what its page lists. */
  endSession(to) {
    const book = this.book;
    const from = book.openedAt;
    book.openedAt = null;
    if (!Number.isFinite(from)) return;
    const within = entry => entry.t >= from && entry.t <= to;
    const sales = book.sales.filter(within);
    book.sessions.unshift({
      from,
      to,
      ...(book.owner ? { owner: book.owner } : {}),
      sold: sales.length,
      earned: sales.reduce((sum, sale) => sum + (sale.mesos || 0), 0),
      visitors: new Set(book.visits.filter(visit => visit.in && within(visit)).map(visit => visit.name)).size,
    });
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
    this.book.sessions = this.book.sessions.filter(session => now - session.to < MAX_AGE_MS).slice(0, KEEP_SESSIONS);
  }

  save() {
    this.storage(s => s.setItem(STORAGE_KEY, JSON.stringify(this.book)));
  }

  /** What the phone gets: the book, when it was made, and whether the page is following the store at all. */
  snapshot(following, now = Date.now()) {
    return { v: 1, at: now, following, ...this.book };
  }
}
