// npm test — the relay's routes against in-memory Durable Objects and a fake push service: what a stranger can and
// cannot do with or without a key.
import assert from "node:assert/strict";
import test from "node:test";

import relay, { StoreState } from "../src/index.js";
import { b64url } from "../src/webpush.js";

const SITE = "https://maplelens.github.io";
const KEY = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";
const WRONG = "AbCdEfGhIjKlMnOp.zzzzzzzzzzzzzzzzzzzzzzzzzz";

/** Durable Object storage, the part of its API the relay uses. */
class ObjectStorage {
  constructor() {
    this.data = new Map();
    this.alarm = null;
    this.writes = 0;
  }
  async get(key) {
    if (Array.isArray(key)) return new Map(key.filter(k => this.data.has(k)).map(k => [k, this.data.get(k)]));
    return this.data.get(key);
  }
  async put(entries) {
    this.writes++;
    for (const [k, v] of Object.entries(entries)) this.data.set(k, structuredClone(v));
  }
  async delete(key) {
    this.writes++;
    for (const k of [key].flat()) this.data.delete(k);
  }
  async deleteAll() {
    this.data.clear();
  }
  async getAlarm() {
    return this.alarm;
  }
  async setAlarm(at) {
    this.writes++;
    this.alarm = at;
  }
  async deleteAlarm() {
    this.alarm = null;
  }
}

class Objects {
  constructor() {
    this.instances = new Map();
  }
  idFromName(name) {
    return name;
  }
  get(id) {
    if (!this.instances.has(id)) this.instances.set(id, new StoreState({ storage: new ObjectStorage() }));
    const instance = this.instances.get(id);
    return { fetch: request => instance.fetch(request) };
  }
  /** Everything every object has stored, as text. */
  dump() {
    return JSON.stringify([...this.instances.values()].map(o => [...o.storage.data.entries()]));
  }
  get writes() {
    return [...this.instances.values()].reduce((n, o) => n + o.storage.writes, 0);
  }
}

class Limiter {
  constructor(limit) {
    this.limit_ = limit;
    this.counts = new Map();
  }
  async limit({ key }) {
    const n = (this.counts.get(key) || 0) + 1;
    this.counts.set(key, n);
    return { success: n <= this.limit_ };
  }
}

async function environment(limits = {}) {
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  return {
    STORES: new Objects(),
    STATE_LIMIT: new Limiter(limits.state ?? 1000),
    CLIENT_LIMIT: new Limiter(limits.client ?? 1000),
    PAIR_LIMIT: new Limiter(limits.pair ?? 1000),
    NOTIFY_LIMIT: new Limiter(limits.notify ?? 1000),
    ALLOWED_ORIGINS: SITE,
    VAPID_SUBJECT: `${SITE}/maplelens/`,
    VAPID_PRIVATE_JWK: JSON.stringify(await crypto.subtle.exportKey("jwk", pair.privateKey)),
    VAPID_PUBLIC: b64url(new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey))),
  };
}

/** A phone's subscription as a browser makes one. */
async function subscription(name) {
  const keys = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]);
  return {
    endpoint: `https://fcm.googleapis.com/fcm/send/${name}`,
    keys: {
      p256dh: b64url(new Uint8Array(await crypto.subtle.exportKey("raw", keys.publicKey))),
      auth: b64url(crypto.getRandomValues(new Uint8Array(16))),
    },
  };
}

/** Every push the relay makes, by endpoint; the fake push service answers `answer(endpoint)` (201 by default). */
function pushService(answer = () => 201) {
  const pushed = [];
  globalThis.fetch = async (url, init) => {
    pushed.push({ url: String(url), headers: init.headers, body: init.body });
    return new Response(null, { status: answer(String(url)) });
  };
  return pushed;
}

async function call(env, path, body, { origin = SITE, ip = "203.0.113.1", raw } = {}) {
  const headers = { "Content-Type": "application/json", "CF-Connecting-IP": ip };
  if (origin) headers.Origin = origin;
  const response = await relay.fetch(
    new Request(`https://relay.example${path}`, { method: "POST", headers, body: raw ?? JSON.stringify(body) }),
    env,
  );
  return { status: response.status, json: await response.json() };
}

test("a paired phone gets what the key holder sends, encrypted", async () => {
  const env = await environment();
  const pushed = pushService();
  assert.equal((await call(env, "/pair", { key: KEY, subscription: await subscription("a") })).status, 200);
  const sent = await call(env, "/notify", { key: KEY, title: "Sold: Pet Food", body: "Bought by Buyer" });
  assert.deepEqual(sent, { status: 200, json: { sent: 1, phones: 1 } });
  assert.equal(pushed.length, 1);
  assert.equal(pushed[0].headers["Content-Encoding"], "aes128gcm");
  assert.ok(!new TextDecoder().decode(pushed[0].body).includes("Pet Food"), "the push service sees only ciphertext");
});

test("nothing is stored but the secret's hash and the subscriptions", async () => {
  const env = await environment();
  pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  await call(env, "/notify", { key: KEY, title: "Sold: Pet Food", body: "Bought by Buyer" });
  const stored = env.STORES.dump();
  assert.ok(!stored.includes(KEY.split(".")[1]), "the secret is not stored");
  assert.ok(!stored.includes("Pet Food") && !stored.includes("Buyer"), "no message is stored");
});

test("a wrong secret can neither send, read, unpair nor join, and looks like no pairing", async () => {
  const env = await environment();
  const pushed = pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("owner") });
  const nothing = { status: 404, json: { error: "not paired" } };
  assert.deepEqual(await call(env, "/notify", { key: WRONG, title: "spam" }), nothing);
  assert.deepEqual(await call(env, "/unpair", { key: WRONG, remove: true }), nothing);
  assert.deepEqual(await call(env, "/unpair", { key: WRONG, endpoint: "https://fcm.googleapis.com/fcm/send/owner" }), nothing);
  assert.deepEqual(await call(env, "/pair", { key: WRONG, subscription: await subscription("thief") }), nothing);
  assert.deepEqual(await call(env, "/status", { key: WRONG }), { status: 200, json: { phones: 0 } });
  assert.deepEqual(await call(env, "/notify", { key: "AAAAAAAAAAAAAAAAAAAAAA.never-paired-secret-xxxxxx", title: "x" }), nothing);
  assert.equal(pushed.length, 0);
  assert.deepEqual(await call(env, "/status", { key: KEY }), { status: 200, json: { phones: 1 } });
});

test("a phone joining a pairing is announced to the phones already in it", async () => {
  const env = await environment();
  const pushed = pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("owner") });
  assert.equal(pushed.length, 0, "the first phone is not announced");
  await call(env, "/pair", { key: KEY, subscription: await subscription("second") });
  assert.deepEqual(pushed.map(p => p.url), ["https://fcm.googleapis.com/fcm/send/owner"]);
});

test("re-pairing an unchanged phone writes nothing and announces nothing", async () => {
  const env = await environment();
  const pushed = pushService();
  const phone = await subscription("owner");
  await call(env, "/pair", { key: KEY, subscription: phone });
  await call(env, "/pair", { key: KEY, subscription: await subscription("second") });
  const writes = env.STORES.writes;
  const alerts = pushed.length;
  await call(env, "/pair", { key: KEY, subscription: phone });
  assert.equal(env.STORES.writes, writes);
  assert.equal(pushed.length, alerts);
});

test("subscriptions must point at a browser's push service", async () => {
  const env = await environment();
  const pushed = pushService();
  const good = await subscription("a");
  for (const endpoint of [
    "https://evil.example/push",
    "http://fcm.googleapis.com/fcm/send/a",
    "https://fcm.googleapis.com.evil.example/a",
    "https://user@fcm.googleapis.com/a",
    "https://fcm.googleapis.com:8443/a",
    "https://169.254.169.254/latest",
    "https://fcm.googleapis.com\\@evil.example/x",
    "https://FCM.googleapis.com/fcm/send/a",
    "https://fcm.googleapis.com/fcm/send/a b",
  ]) {
    const answer = await call(env, "/pair", { key: KEY, subscription: { ...good, endpoint } });
    assert.deepEqual(answer, { status: 400, json: { error: "subscription" } }, endpoint);
  }
  assert.equal((await call(env, "/pair", { key: KEY, subscription: { ...good, keys: { p256dh: "x", auth: "y" } } })).status, 400);
  for (const endpoint of ["https://web.push.apple.com/QGabc", "https://updates.push.services.mozilla.com/wpush/v2/abc", "https://wns2-par02p.notify.windows.com/w/?token=abc"]) {
    assert.equal((await call(env, "/pair", { key: KEY, subscription: { ...good, endpoint } })).status, 200, endpoint);
  }
  // Each new phone alerts the ones before it: 0 + 1 + 2.
  assert.equal(pushed.length, 3);
});

test("icons come from the item picture host only", async () => {
  const env = await environment();
  const pushed = pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  // Decrypting is webpush.test.mjs's job; the payload's size tells whether the icon went along.
  await call(env, "/notify", { key: KEY, title: "t", icon: "https://evil.example/track.png" });
  await call(env, "/notify", { key: KEY, title: "t" });
  await call(env, "/notify", { key: KEY, title: "t", icon: "https://maplestory.io/api/GMS/40/item/2120000/icon" });
  const [tracked, plain, item] = pushed.map(p => p.body.length);
  assert.equal(tracked, plain);
  assert.ok(item > plain);
});

test("bad requests are refused before any lookup", async () => {
  const env = await environment();
  pushService();
  assert.equal((await call(env, "/notify", null, { raw: "x".repeat(9000) })).status, 400);
  assert.equal((await call(env, "/notify", null, { raw: "[1,2]" })).status, 400);
  assert.equal((await call(env, "/notify", { key: "a.b", title: "x" })).status, 400);
  assert.equal((await call(env, "/notify", { key: KEY, title: "x" }, { origin: "https://evil.example" })).status, 403);
  assert.equal((await call(env, "/nowhere", {})).status, 404);
  assert.equal(env.STORES.instances.size, 0);
});

test("rate limits: per address for everything and for pairing, per pairing for sending", async () => {
  const env = await environment({ client: 6, pair: 2, notify: 3 });
  pushService();
  const pairs = [];
  for (let i = 0; i < 3; i++) pairs.push((await call(env, "/pair", { key: KEY, subscription: await subscription(`p${i}`) })).status);
  assert.deepEqual(pairs, [200, 200, 429]);
  // Another address sends with the key: the pairing's own limit counts, whoever sends.
  const sends = [];
  for (let i = 0; i < 4; i++) sends.push((await call(env, "/notify", { key: KEY, title: "x" }, { ip: "198.51.100.7" })).status);
  assert.deepEqual(sends, [200, 200, 200, 429]);
  // The first address has used 3 of its 6; junk keys still count against it.
  const junk = [];
  for (let i = 0; i < 4; i++) junk.push((await call(env, "/status", { key: KEY.replace("Ab", `X${i}`) })).status);
  assert.deepEqual(junk, [200, 200, 200, 429]);
});

test("a phone the push service no longer knows is forgotten", async () => {
  const env = await environment();
  pushService(url => (url.endsWith("/gone") ? 410 : 201));
  await call(env, "/pair", { key: KEY, subscription: await subscription("gone") });
  await call(env, "/pair", { key: KEY, subscription: await subscription("here") });
  assert.deepEqual((await call(env, "/notify", { key: KEY, title: "x" })).json, { sent: 1, phones: 1 });
  assert.deepEqual((await call(env, "/status", { key: KEY })).json, { phones: 1 });
});

test("the sealed store state goes from MapleLens to the phones, and to nobody else", async () => {
  const env = await environment();
  pushService();
  const blob = "c2VhbGVkLWJ5LXRoZS1wYWdl";
  assert.deepEqual((await call(env, "/state/put", { key: KEY, blob })).status, 404, "no pairing yet: nothing is kept");
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  assert.deepEqual(await call(env, "/state/put", { key: KEY, blob }), { status: 200, json: { phones: 1 } });
  const got = await call(env, "/state/get", { key: KEY });
  assert.equal(got.json.blob, blob);
  assert.equal(got.json.phones, 1);
  assert.ok(got.json.seen >= got.json.at);
  const nothing = { status: 404, json: { error: "not paired" } };
  assert.deepEqual(await call(env, "/state/get", { key: WRONG }), nothing);
  assert.deepEqual(await call(env, "/state/put", { key: WRONG, blob: "x" }), nothing);
  assert.equal((await call(env, "/state/get", { key: KEY })).json.blob, blob, "a wrong key changed nothing");
});

test("a store state must be a sealed blob of bounded size", async () => {
  const env = await environment();
  pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  assert.equal((await call(env, "/state/put", { key: KEY, blob: { not: "a string" } })).status, 400);
  assert.equal((await call(env, "/state/put", { key: KEY, blob: "plain text, not base64url" })).status, 400);
  assert.equal((await call(env, "/state/put", { key: KEY, blob: "A".repeat(160 * 1024) })).status, 400);
});

test("removing on the computer tells every phone and forgets the state; a phone leaving tells nobody", async () => {
  const env = await environment();
  const pushed = pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  await call(env, "/pair", { key: KEY, subscription: await subscription("b") });
  await call(env, "/state/put", { key: KEY, blob: "AAAA" });
  pushed.length = 0;
  await call(env, "/unpair", { key: KEY, endpoint: "https://fcm.googleapis.com/fcm/send/b" });
  assert.equal(pushed.length, 0);
  assert.equal((await call(env, "/state/ping", { key: KEY })).json.phones, 1, "the computer sees the phone count drop");
  await call(env, "/unpair", { key: KEY, remove: true });
  assert.deepEqual(pushed.map(p => p.url), ["https://fcm.googleapis.com/fcm/send/a"]);
  assert.deepEqual(await call(env, "/state/get", { key: KEY }), { status: 410, json: { error: "removed" } });
  const left = env.STORES.instances.get(KEY.split(".")[0]).storage.data;
  assert.deepEqual([...left.keys()].sort(), ["hash", "removed"], "the sealed state is deleted, a tombstone stays");
});

test("a removed key stays removed: no re-pairing, nothing sent, and nothing told to others", async () => {
  const env = await environment();
  const pushed = pushService();
  const phone = await subscription("a");
  await call(env, "/pair", { key: KEY, subscription: phone });
  await call(env, "/unpair", { key: KEY, remove: true });
  pushed.length = 0;
  // The phone app re-pairs on start with the key it still holds.
  assert.deepEqual(await call(env, "/pair", { key: KEY, subscription: phone }), { status: 410, json: { error: "removed" } });
  assert.equal((await call(env, "/state/get", { key: KEY })).status, 410);
  assert.deepEqual(await call(env, "/notify", { key: KEY, title: "x" }), { status: 410, json: { error: "removed" } });
  assert.equal((await call(env, "/status", { key: KEY })).status, 410);
  assert.equal(pushed.length, 0);
  // A wrong secret learns nothing about the tombstone.
  assert.deepEqual(await call(env, "/state/get", { key: WRONG }), { status: 404, json: { error: "not paired" } });
});

test("an unpair naming no phone is refused, not read as a removal", async () => {
  const env = await environment();
  const pushed = pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  await call(env, "/pair", { key: KEY, subscription: await subscription("b") });
  pushed.length = 0;
  for (const body of [{ key: KEY }, { key: KEY, endpoint: null }, { key: KEY, remove: "yes" }]) {
    assert.deepEqual(await call(env, "/unpair", body), { status: 400, json: { error: "endpoint" } });
  }
  assert.equal(pushed.length, 0);
  assert.deepEqual((await call(env, "/status", { key: KEY })).json, { phones: 2 });
});

test("a pairing whose last phone uninstalled says so, and can pair again", async () => {
  const env = await environment();
  pushService(url => (url.endsWith("/gone") ? 410 : 201));
  await call(env, "/pair", { key: KEY, subscription: await subscription("gone") });
  assert.deepEqual(await call(env, "/notify", { key: KEY, title: "x" }), { status: 404, json: { sent: 0, phones: 0 } });
  assert.equal((await call(env, "/pair", { key: KEY, subscription: await subscription("new") })).status, 200);
  assert.equal((await call(env, "/state/ping", { key: KEY })).json.phones, 1);
});

test("a phone pairing while a message is sent stays paired when a dead phone is dropped", async () => {
  const env = await environment();
  const late = await subscription("late");
  let joining;
  const pushed = pushService(url => {
    // The push to the dead phone is answered after another phone has paired.
    if (url.endsWith("/gone") && !joining) joining = call(env, "/pair", { key: KEY, subscription: late });
    return url.endsWith("/gone") ? 410 : 201;
  });
  await call(env, "/pair", { key: KEY, subscription: await subscription("gone") });
  await call(env, "/pair", { key: KEY, subscription: await subscription("here") });
  pushed.length = 0;
  joining = undefined;
  await call(env, "/notify", { key: KEY, title: "x" });
  await joining;
  pushed.length = 0;
  await call(env, "/notify", { key: KEY, title: "y" });
  const urls = pushed.map(p => p.url).sort();
  assert.deepEqual(urls, ["https://fcm.googleapis.com/fcm/send/here", "https://fcm.googleapis.com/fcm/send/late"]);
});

test("a phone renewing its subscription replaces its old one without a new-phone alert", async () => {
  const env = await environment();
  const pushed = pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("owner") });
  await call(env, "/pair", { key: KEY, subscription: await subscription("old") });
  pushed.length = 0;
  const renewed = { key: KEY, subscription: await subscription("renewed"), replaces: "https://fcm.googleapis.com/fcm/send/old" };
  assert.deepEqual(await call(env, "/pair", renewed), { status: 200, json: { phones: 2 } });
  assert.equal(pushed.length, 0, "nobody was alerted");
  // Naming an endpoint that is not in the pairing makes it a new phone like any other.
  const other = { key: KEY, subscription: await subscription("other"), replaces: "https://fcm.googleapis.com/fcm/send/nope" };
  await call(env, "/pair", other);
  assert.equal(pushed.length, 2);
});

test("once a secret has claimed an id, another secret cannot pair, even with no phone left", async () => {
  const env = await environment();
  pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("owner") });
  await call(env, "/unpair", { key: KEY, endpoint: "https://fcm.googleapis.com/fcm/send/owner" });
  const answer = await call(env, "/pair", { key: WRONG, subscription: await subscription("thief") });
  assert.deepEqual(answer, { status: 404, json: { error: "not paired" } });
  assert.equal((await call(env, "/pair", { key: KEY, subscription: await subscription("owner") })).status, 200);
});

test("a full pairing refuses a new phone instead of dropping one", async () => {
  const env = await environment();
  const pushed = pushService();
  for (let i = 0; i < 5; i++) await call(env, "/pair", { key: KEY, subscription: await subscription(`p${i}`) });
  pushed.length = 0;
  const sixth = await call(env, "/pair", { key: KEY, subscription: await subscription("sixth") });
  assert.deepEqual(sixth, { status: 409, json: { error: "full", phones: 5 } });
  assert.equal(pushed.length, 0, "nobody was alerted");
  await call(env, "/notify", { key: KEY, title: "x" });
  assert.deepEqual(pushed.map(p => p.url).sort(), [0, 1, 2, 3, 4].map(i => `https://fcm.googleapis.com/fcm/send/p${i}`));
  // A phone already in it can still renew.
  const renewed = { key: KEY, subscription: await subscription("p0b"), replaces: "https://fcm.googleapis.com/fcm/send/p0" };
  assert.equal((await call(env, "/pair", renewed)).status, 200);
});

test("a pairing left unused for 60 days is forgotten; a removed key's tombstone stays", async () => {
  const env = await environment();
  pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  const object = env.STORES.instances.get(KEY.split(".")[0]);
  const days = (object.storage.alarm - Date.now()) / 86400000;
  assert.ok(days > 59 && days <= 60, `alarm in ${days} days`);
  // MapleLens checking in every minute pushes the alarm back once a day, not every time.
  const writes = object.storage.writes;
  await call(env, "/state/ping", { key: KEY });
  await call(env, "/state/ping", { key: KEY });
  assert.equal(object.storage.writes, writes);
  await object.alarm();
  assert.equal(object.storage.data.size, 0);
  assert.equal((await call(env, "/state/ping", { key: KEY })).status, 404);
  // Removed: no alarm left to take the tombstone away.
  await call(env, "/pair", { key: KEY, subscription: await subscription("b") });
  await call(env, "/unpair", { key: KEY, remove: true });
  assert.equal(object.storage.alarm, null);
});

test("nobody but the relay can send its own tags (a phone forgets its key on \"unpaired\")", async () => {
  const env = await environment();
  const pushed = pushService();
  await call(env, "/pair", { key: KEY, subscription: await subscription("a") });
  await call(env, "/notify", { key: KEY, title: "t" });
  await call(env, "/notify", { key: KEY, title: "t", tag: "unpaired" });
  await call(env, "/notify", { key: KEY, title: "t", tag: "new-phone" });
  await call(env, "/notify", { key: KEY, title: "t", tag: "store" });
  const [plain, unpaired, newPhone, store] = pushed.map(p => p.body.length);
  assert.equal(unpaired, plain, "the reserved tag was dropped");
  assert.equal(newPhone, plain);
  assert.ok(store > plain, "an ordinary tag goes through");
});

test("a body with no Content-Length is cut off at the limit, not read whole", async () => {
  const env = await environment();
  const chunk = new TextEncoder().encode("x".repeat(4096));
  let sent = 0;
  const body = new ReadableStream({
    pull(controller) {
      sent += 1;
      if (sent > 1000) controller.close();
      else controller.enqueue(chunk);
    },
  });
  const request = new Request("https://relay.example/notify", { method: "POST", body, duplex: "half", headers: { "CF-Connecting-IP": "203.0.113.9" } });
  const response = await relay.fetch(request, env);
  assert.equal(response.status, 400);
  assert.ok(sent < 10, `read ${sent} chunks of 4 KB before stopping`);
});
