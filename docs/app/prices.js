// Prices from MesoWatch, kept in this browser. Used with MesoWatch's written permission on one condition: the price
// list is asked for at most once every 30 minutes, and between those asks the page reads the copy in localStorage. So
// every request (a success or a failure, from any tab, however often the page is opened) is stamped first, and no new
// one goes out until the stamp is 30 minutes old.

/** MapleLens's relay (a Cloudflare Worker kept outside this repo): MesoWatch's list.json sends no CORS headers, so a page
 *  cannot read it directly. The relay asks MesoWatch at most every 30 minutes itself and is rate limited per client.
 *  `?prices=URL` points a test page at a copy instead, on a page served from this computer only: on the site, a link
 *  carrying it would have the player's page keep someone else's prices (and use up its one request in 30 minutes). */
export const PRICES_URL = "https://prices.maple.observer/prices.json";
const LOCAL = /^(localhost|127\.0\.0\.1|\[::1\])$/;

/** Where the list comes from: the relay, or a test page's own copy. */
export function pricesUrl(location) {
  const asked = LOCAL.test(location.hostname) && new URLSearchParams(location.search).get("prices");
  return asked || PRICES_URL;
}
/** The permission's condition: never more often than this. */
export const MIN_GAP_MS = 30 * 60 * 1000;
const CACHE_KEY = "maplelens.prices.v1";
const ATTEMPT_KEY = "maplelens.prices.attempt";
/** The list is about 2 MB; a reply far bigger is not it. */
const MAX_CHARS = 32 * 1024 * 1024;

function storage(action) {
  try { return action(window.localStorage); } catch { return undefined; } // private windows and full disks refuse
}

function readCache() {
  try {
    const cached = JSON.parse(storage(s => s.getItem(CACHE_KEY)) || "null");
    return cached && typeof cached.text === "string" && Number.isFinite(cached.fetchedAt) ? cached : null;
  } catch {
    return null;
  }
}

function lastAttempt() {
  return Number(storage(s => s.getItem(ATTEMPT_KEY))) || 0;
}

export class Prices {
  /** @param {(json: string) => Promise<number>} load  hands a price list to the engine; resolves to its item count */
  constructor(load, { url = pricesUrl(location), now = () => Date.now() } = {}) {
    this.load = load;
    this.url = url;
    this.now = now;
    this.fetchedAt = 0;
    this.items = 0;
    this.error = "";
    this.busy = false;
  }

  /** Is a request allowed yet? */
  due() {
    return this.now() - lastAttempt() >= MIN_GAP_MS;
  }

  /** Use what is on hand (the cached copy, however old), then ask again if 30 minutes have passed. Returns true
   *  when prices from either source are loaded. */
  async refresh() {
    if (this.busy) return Boolean(this.items);
    this.busy = true;
    try {
      const cached = readCache();
      if (cached && !this.items) await this.use(cached.text, cached.fetchedAt);
      const fresh = cached && this.now() - cached.fetchedAt < MIN_GAP_MS;
      if (!fresh && this.due()) await this.fetchNew();
      return Boolean(this.items);
    } finally {
      this.busy = false;
    }
  }

  async use(text, fetchedAt) {
    try {
      this.items = await this.load(text);
      this.fetchedAt = fetchedAt;
      return true;
    } catch (error) {
      this.error = `the price list could not be read (${error?.message || error})`;
      return false;
    }
  }

  async fetchNew() {
    // Stamped before asking: a page that crashes, a failure and a second tab all count as the one request.
    storage(s => s.setItem(ATTEMPT_KEY, String(this.now())));
    try {
      const response = await fetch(this.url, { credentials: "omit", cache: "no-cache" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const text = await response.text();
      if (text.length > MAX_CHARS) throw new Error("reply too large");
      if (await this.use(text, this.now())) {
        this.error = "";
        // Kept only once the engine accepted it, so a bad reply never replaces a good copy.
        storage(s => s.setItem(CACHE_KEY, JSON.stringify({ fetchedAt: this.fetchedAt, text })));
      }
    } catch (error) {
      this.error = this.items
        ? "The price service could not be reached; using the last copy"
        : `The price service could not be reached (${error?.message || error}); showing demo prices`;
    }
  }

  /** Minutes until a request is allowed again (0 when one is). */
  waitMinutes() {
    return Math.max(0, Math.ceil((MIN_GAP_MS - (this.now() - lastAttempt())) / 60000));
  }

  /** "updated 12 min ago" for the page. */
  age() {
    const minutes = Math.max(0, Math.round((this.now() - this.fetchedAt) / 60000));
    return minutes < 1 ? "just now" : minutes < 90 ? `${minutes} min ago` : `${Math.round(minutes / 60)} h ago`;
  }
}
