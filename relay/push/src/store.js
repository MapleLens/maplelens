// One pairing: a Durable Object holding the secret's hash, the paired phones' push subscriptions, and the sealed
// snapshot of the store MapleLens uploads for the phone app. It is the pairing's only record: a Durable Object runs
// one request at a time with strongly consistent storage, so two phones pairing at once both stay, and a phone that
// just paired is seen by the next /notify from anywhere (KV, used before, cached a miss for a minute per location and
// lost a phone to a concurrent write). The snapshot is sealed by the page with a key derived from the pairing secret
// (docs/app/phone/relay.js `seal`); this object only ever sees ciphertext.

/** Phones one pairing can hold (a player's phone and tablet; more is someone sharing a QR). */
const MAX_PHONES = 5;
/** A snapshot nobody refreshed for this long is deleted: the store is long closed or the player stopped using it. */
const KEEP_MS = 14 * 24 * 3600 * 1000;

export class StoreState {
  constructor(ctx) {
    this.ctx = ctx;
    this.storage = ctx.storage;
    /** When MapleLens last checked in (memory only: a ping must not cost a storage write). */
    this.seen = 0;
  }

  /** `{op, hash, ...}` from the Worker, which has checked the key's shape, the subscription and the rate limits. */
  async fetch(request) {
    const { op, hash, phone, replaces, endpoint, endpoints, blob } = await request.json();
    const reply = (status, json) => Response.json(json, { status });
    const meta = await this.storage.get(["hash", "phones", "removed"]);
    const known = meta.get("hash");
    if (meta.get("removed")) {
      // Only the key holder learns it was removed; anyone else sees what a missing pairing looks like.
      return known === hash ? reply(410, { error: "removed" }) : reply(404, { error: "not paired" });
    }
    // Once a secret has claimed this id, only that secret gets through.
    if (known && known !== hash) return reply(404, { error: "not paired" });
    const phones = Array.isArray(meta.get("phones")) ? meta.get("phones") : [];
    const now = Date.now();
    if (op === "pair") return this.pair(hash, phones, phone, replaces, reply);
    if (!known) return reply(404, { error: "not paired" });
    switch (op) {
      case "phones":
        return reply(200, { phones });
      case "leave":
      case "drop": {
        // A phone left by itself, or the push service no longer knows it: the key stays good for pairing again.
        const gone = op === "leave" ? [endpoint] : endpoints;
        const left = phones.filter(p => !gone.includes(p.endpoint));
        if (left.length < phones.length) await this.storage.put({ phones: left });
        return reply(200, { phones: left.length });
      }
      case "remove":
        // Removed in MapleLens: this key never works again (a few bytes, kept for good: a phone that missed the
        // "disconnected" push may come back with the key months later). The phones are handed back to be told.
        await this.forget();
        await this.storage.put({ removed: true, hash });
        return reply(200, { phones });
      case "put":
        await this.storage.put({ blob, at: now });
        this.seen = now;
        await this.storage.setAlarm(now + KEEP_MS);
        return reply(200, { phones: phones.length });
      case "ping":
        this.seen = now;
        return reply(200, { phones: phones.length });
      case "get": {
        const stored = await this.storage.get(["blob", "at"]);
        const at = stored.get("at") || null;
        const seen = Math.max(this.seen, at || 0) || null;
        return reply(200, { blob: stored.get("blob") || null, at, seen, phones: phones.length });
      }
      default:
        return reply(400, { error: "op" });
    }
  }

  /** A phone joins, or renews its subscription (`replaces`: the endpoint it had). Answers the phones to alert. */
  async pair(hash, phones, phone, replaces, reply) {
    const known = phones.find(p => p.endpoint === phone.endpoint);
    // The phone page re-pairs on every start: an unchanged phone writes nothing.
    if (known && known.keys.p256dh === phone.keys.p256dh && known.keys.auth === phone.keys.auth) {
      return reply(200, { phones: phones.length, alert: [] });
    }
    const renewed = !known && typeof replaces === "string" && phones.some(p => p.endpoint === replaces);
    const others = phones.filter(p => p.endpoint !== phone.endpoint && p.endpoint !== replaces);
    // The oldest phone goes first: one that reinstalled the app leaves a dead subscription behind.
    const kept = [...others, phone].slice(-MAX_PHONES);
    await this.storage.put({ hash, phones: kept });
    // A QR code seen on a stream pairs a stranger's phone too: the phones already paired hear about every new one.
    // A phone renewing its own subscription, or one whose keys changed, is not new.
    return reply(200, { phones: kept.length, alert: known || renewed ? [] : others });
  }

  async alarm() {
    // The snapshot is stale; a pairing that has no phone left goes with it.
    const phones = await this.storage.get("phones");
    if (Array.isArray(phones) && phones.length) await this.storage.delete(["blob", "at"]);
    else await this.forget();
  }

  async forget() {
    this.seen = 0;
    await this.storage.deleteAlarm();
    await this.storage.deleteAll();
  }
}
