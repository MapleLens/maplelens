// One pairing's store state: a Durable Object holding the sealed snapshot MapleLens uploads for the phone app, the
// pairing's secret hash (so the routes it serves need no KV read) and how many phones are paired. The snapshot is
// sealed by the page with a key derived from the pairing secret (docs/app/phone/relay.js `seal`); this object only
// ever sees ciphertext. It is also the authority on a removed pairing: KV is eventually consistent, so a pairing
// removed in MapleLens could be read back from a stale copy for a minute; the tombstone here refuses it for good.

/** A snapshot nobody refreshed for this long is deleted: the store is long closed or the player stopped using it. */
const KEEP_MS = 14 * 24 * 3600 * 1000;
/** After the last phone went away, KV may still show it this long: no adopting from KV meanwhile. */
const KV_SETTLE_MS = 2 * 60 * 1000;

export class StoreState {
  constructor(ctx) {
    this.ctx = ctx;
    this.storage = ctx.storage;
    /** When MapleLens last checked in (memory only: a ping must not cost a storage write). */
    this.seen = 0;
  }

  /** `{op, hash, ...}` from the Worker, which has checked the key's shape and the rate limits. */
  async fetch(request) {
    const { op, hash, phones, blob } = await request.json();
    const reply = (status, json) => Response.json(json, { status });
    const meta = await this.storage.get(["hash", "phones", "removed", "emptiedAt"]);
    const now = Date.now();
    if (meta.get("removed")) {
      // Only the key holder learns it was removed; anyone else sees what a missing pairing looks like.
      return meta.get("hash") === hash ? reply(410, { error: "removed" }) : reply(404, { error: "not paired" });
    }
    switch (op) {
      case "sync":
        // The Worker just wrote this pairing to KV: adopt its hash and phone count.
        await this.storage.put({ hash, phones });
        await this.storage.delete("emptiedAt");
        return reply(200, {});
      case "adopt":
        // The Worker read the pairing from KV (this object had not met it): refused while KV may be stale.
        if (now - (meta.get("emptiedAt") || 0) < KV_SETTLE_MS) return reply(404, { error: "not paired" });
        await this.storage.put({ hash, phones });
        return reply(200, {});
      case "check":
        // Once this object knows a pairing, only its secret gets through (KV elsewhere may not have caught up).
        return meta.get("hash") && meta.get("hash") !== hash ? reply(404, { error: "not paired" }) : reply(200, {});
      case "empty":
        // The last phone went away by itself (uninstalled): the key may pair again.
        await this.forget();
        await this.storage.put({ emptiedAt: now });
        return reply(200, {});
      case "remove":
        // Removed in MapleLens: this key never works again (a few bytes, kept for good: a phone that missed the
        // "disconnected" push may come back with the key months later).
        await this.forget();
        await this.storage.put({ removed: true, hash });
        return reply(200, {});
    }
    const known = meta.get("hash");
    if (!known) return reply(409, { error: "unknown" });
    if (known !== hash) return reply(404, { error: "not paired" });
    const count = meta.get("phones") || 0;
    switch (op) {
      case "put":
        await this.storage.put({ blob, at: now });
        this.seen = now;
        await this.storage.setAlarm(now + KEEP_MS);
        return reply(200, { phones: count });
      case "ping":
        this.seen = now;
        return reply(200, { phones: count });
      case "get": {
        const stored = await this.storage.get(["blob", "at"]);
        const at = stored.get("at") || null;
        return reply(200, { blob: stored.get("blob") || null, at, seen: Math.max(this.seen, at || 0) || null, phones: count });
      }
      default:
        return reply(400, { error: "op" });
    }
  }

  async alarm() {
    await this.forget();
  }

  async forget() {
    this.seen = 0;
    await this.storage.deleteAlarm();
    await this.storage.deleteAll();
  }
}
