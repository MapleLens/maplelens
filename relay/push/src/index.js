// MapleLens phone notifications: a Cloudflare Worker between MapleLens (the Windows app or the web page) and the
// player's phone. The phone page subscribes to Web Push and pairs with a key from a QR code; MapleLens then posts a
// title and a line of text with the same key and the Worker pushes it, encrypted, to the phone's push service.
//
//   GET  /key                          the VAPID public key (base64url), for PushManager.subscribe
//   POST /pair     {key, subscription, replaces?}  a phone joins a pairing (at most 5); the others are told, unless
//                                      it renews its own subscription (`replaces`: the endpoint it had)
//   POST /unpair   {key, endpoint}     a phone leaves it
//   POST /unpair   {key, remove: true} MapleLens removes the pairing: every phone is told, the key never works again
//   POST /notify   {key, title, body, icon?, tag?}
//   POST /status   {key}               how many phones are paired (MapleLens hides its QR code once one is)
//   POST /state/put  {key, blob}       MapleLens uploads the store's state, sealed for the phones (store.js)
//   POST /state/ping {key}             MapleLens is still watching; answers how many phones are paired
//   POST /state/get  {key}             the phone app reads the sealed state, and when MapleLens last checked in
//
// The key is the only credential: "<id>.<secret>", 96 + 128 random bits made by MapleLens. The id names the pairing's
// Durable Object (store.js), which keeps only the secret's SHA-256, so a leaked backup cannot send. A wrong secret
// looks exactly like no pairing at all. Nothing about a message is stored or logged. See README.md for deploying and the threat model.

import { StoreState } from "./store.js";
import { encryptPayload, vapidHeader } from "./webpush.js";

export { StoreState };

/** What a notification may carry: the push services take ~4 KB encrypted. */
const TITLE_CHARS = 120;
const BODY_CHARS = 600;
const TAG_CHARS = 64;
/** Bigger requests are refused unread: a pairing or a message is well under 2 KB. */
const MAX_BODY_BYTES = 8 * 1024;
/** A sealed store state: a week of a busy store's history is ~60 KB as base64. */
const MAX_STATE_CHARS = 150 * 1024;
/** Item pictures come from this host only (`notify::item_icon_url`): the phone fetches the icon, and an icon on any
 *  other server would tell that server the phone's address. */
const ICON_PREFIX = "https://maplestory.io/api/";
/** The push service drops a message the phone has not fetched after this long: a sale an hour later is still news. */
const TTL_S = 6 * 3600;
const KEY = /^([A-Za-z0-9_-]{16,32})\.([A-Za-z0-9_-]{22,64})$/;
/** Push endpoints are on the browsers' own services only: a subscription naming any other host is refused, so the
 *  Worker cannot be made to POST to an arbitrary server. */
const PUSH_HOSTS = [
  /^fcm\.googleapis\.com$/,
  /^updates\.push\.services\.mozilla\.com$/,
  /^web\.push\.apple\.com$/,
  /^[a-z0-9-]+\.notify\.windows\.com$/,
];
/** Tags the relay itself sends: the phone acts on "unpaired" (it forgets its key), so nobody else may use them. */
const RESERVED_TAGS = ["unpaired", "new-phone"];
const TAG = /^[a-z0-9-]{1,32}$/;
const ROUTES = ["/pair", "/unpair", "/notify", "/status", "/state/put", "/state/ping", "/state/get"];

export default {
  async fetch(request, env) {
    const origin = request.headers.get("Origin") || "";
    const cors = corsHeaders(origin, env);
    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors });
    const reply = (status, json) =>
      new Response(JSON.stringify(json), {
        status,
        headers: { ...cors, "Content-Type": "application/json", "Cache-Control": "no-store" },
      });
    // A page on another site must not drive the relay from its visitors' browsers; apps send no Origin.
    if (origin && !cors["Access-Control-Allow-Origin"]) return reply(403, { error: "origin" });

    const path = new URL(request.url).pathname;
    if (request.method === "GET" && path === "/key") return reply(200, { key: env.VAPID_PUBLIC });
    if (request.method !== "POST" || !ROUTES.includes(path)) return reply(404, { error: "route" });

    // Every address first, before anything is read or looked up: random keys must not buy storage reads.
    const client = request.headers.get("CF-Connecting-IP") || "unknown";
    if (!(await allowed(env.CLIENT_LIMIT, client))) return slowDown(reply);
    const body = await readJson(request, path === "/state/put" ? MAX_STATE_CHARS + 1024 : MAX_BODY_BYTES);
    if (!body) return reply(400, { error: "body" });
    const key = KEY.exec(typeof body.key === "string" ? body.key : "");
    if (!key) return reply(400, { error: "key" });
    const [, id, secret] = key;
    // Pairing is per address (it writes storage); sending and polling are per pairing (a runaway sender).
    const limiter = path === "/pair" || path === "/unpair" ? env.PAIR_LIMIT : path.startsWith("/state/") ? env.STATE_LIMIT : env.NOTIFY_LIMIT;
    if (!(await allowed(limiter, path === "/pair" || path === "/unpair" ? client : `${path}:${id}`))) return slowDown(reply);

    const hash = await sha256(secret);
    if (path.startsWith("/state/")) return state(env, id, hash, path.slice("/state/".length), body, reply);
    if (path === "/pair") return pair(env, id, hash, body, reply);
    if (path === "/unpair") return unpair(env, id, hash, body, reply);
    // Every key, right or wrong, asks its object once, so the time an answer takes doesn't tell whether an id exists.
    const found = await ask(env, id, "phones", { hash });
    // Someone else's pairing looks exactly like none: no phones, nothing to send to.
    if (path === "/status") {
      if (found.status === 404) return reply(200, { phones: 0 });
      return found.ok ? reply(200, { phones: found.json.phones.length }) : reply(found.status, found.json);
    }
    if (!found.ok) return reply(found.status, found.json);
    return notify(env, id, hash, found.json.phones, message(body), reply);
  },
};

async function pair(env, id, hash, body, reply) {
  const phone = validSubscription(body.subscription);
  if (!phone) return reply(400, { error: "subscription" });
  const replaces = typeof body.replaces === "string" ? body.replaces : undefined;
  const answer = await ask(env, id, "pair", { hash, phone, replaces });
  if (!answer.ok) return reply(answer.status, answer.json);
  if (answer.json.alert.length) {
    const alert = {
      title: "A new phone was connected",
      body: "It gets your store notifications too. Not yours? Press Remove next to Phone in MapleLens, then connect again.",
      tag: "new-phone",
    };
    await Promise.all(answer.json.alert.map(p => push(env, p, encode(alert))));
  }
  return reply(200, { phones: answer.json.phones });
}

/** `pairing`: the id, so a phone acts on the removal of the pairing it holds only (sw.js). */
const removedMessage = id => ({
  title: "Disconnected from MapleLens",
  body: "This phone was removed on your computer. Scan a new code to connect again.",
  tag: "unpaired",
  pairing: id,
});

async function unpair(env, id, hash, body, reply) {
  if (body.remove === true) {
    // MapleLens removed the pairing: every phone hears it, and its app clears itself (sw.js, tag "unpaired").
    const answer = await ask(env, id, "remove", { hash });
    if (!answer.ok) return reply(answer.status, answer.json);
    await Promise.all(answer.json.phones.map(p => push(env, p, encode(removedMessage(id)))));
    return reply(200, { phones: 0 });
  }
  // A phone leaving names itself; a request naming nothing is refused rather than read as a removal.
  if (typeof body.endpoint !== "string") return reply(400, { error: "endpoint" });
  const answer = await ask(env, id, "leave", { hash, endpoint: body.endpoint });
  return reply(answer.status, answer.json);
}

/** The store-state routes, served by the pairing's Durable Object. */
async function state(env, id, hash, action, body, reply) {
  if (action === "put" && (typeof body.blob !== "string" || body.blob.length > MAX_STATE_CHARS || !/^[A-Za-z0-9_-]+$/.test(body.blob))) {
    return reply(400, { error: "blob" });
  }
  const answer = await ask(env, id, action, { hash, blob: body.blob });
  return reply(answer.status, answer.json);
}

/** One request to the pairing's object: `{ok, status, json}`. */
async function ask(env, id, name, fields) {
  const response = await env.STORES.get(env.STORES.idFromName(id)).fetch(op(name, fields));
  return { ok: response.ok, status: response.status, json: await response.json() };
}

function op(name, fields) {
  return new Request("https://store.internal/", { method: "POST", body: JSON.stringify({ op: name, ...fields }) });
}

function message(body) {
  const tag = text(body.tag, TAG_CHARS);
  const out = { title: text(body.title, TITLE_CHARS), body: text(body.body, BODY_CHARS) };
  if (TAG.test(tag) && !RESERVED_TAGS.includes(tag)) out.tag = tag;
  const icon = text(body.icon, 300);
  if (icon.startsWith(ICON_PREFIX)) out.icon = icon;
  return out;
}

async function notify(env, id, hash, phones, content, reply) {
  if (!content.title) return reply(400, { error: "title" });
  const payload = encode(content);
  const results = await Promise.all(phones.map(phone => push(env, phone, payload)));
  const gone = phones.filter((_, i) => results[i] === "gone").map(p => p.endpoint);
  // Only the dead are dropped, by name: a phone that paired while this was sending stays.
  const left = gone.length ? (await ask(env, id, "drop", { hash, endpoints: gone })).json.phones : phones.length;
  const sent = results.filter(r => r === "sent").length;
  // No phone left is "not paired" (410 is kept for a removed key, which the page acts on).
  if (!sent) return reply(left ? 502 : 404, { sent, phones: left });
  return reply(200, { sent, phones: left });
}

/** One push; "sent", "gone" (the phone unsubscribed or uninstalled: forget it) or "failed". */
async function push(env, phone, payload) {
  try {
    const endpoint = new URL(phone.endpoint);
    const response = await fetch(phone.endpoint, {
      method: "POST",
      headers: {
        Authorization: await vapidHeader(env, endpoint.origin),
        "Content-Encoding": "aes128gcm",
        "Content-Type": "application/octet-stream",
        TTL: String(TTL_S),
        Urgency: "high",
      },
      body: await encryptPayload(phone.keys, payload),
      redirect: "manual",
    });
    if (response.status === 404 || response.status === 410) return "gone";
    return response.ok ? "sent" : "failed";
  } catch {
    return "failed";
  }
}

function validSubscription(subscription) {
  const endpoint = typeof subscription?.endpoint === "string" ? subscription.endpoint : "";
  const { p256dh, auth } = subscription?.keys || {};
  let url;
  try {
    url = new URL(endpoint);
  } catch {
    return null;
  }
  if (url.protocol !== "https:" || url.port || url.username || url.password || endpoint.length > 1024) return null;
  // Only an address already in the URL standard's own form (what browsers' PushManager gives): a backslash, a
  // percent-encoded host or a trailing dot that one parser reads one way and another differently is refused.
  if (url.href !== endpoint) return null;
  if (!PUSH_HOSTS.some(host => host.test(url.hostname))) return null;
  // An uncompressed P-256 point (65 bytes) and a 16-byte secret, as every browser makes them.
  if (!/^[A-Za-z0-9_-]{86,88}$/.test(p256dh || "") || !/^[A-Za-z0-9_-]{22,24}$/.test(auth || "")) return null;
  return { endpoint, keys: { p256dh, auth } };
}

/** The JSON body, or null when it is missing, bigger than `limit` bytes or not an object. */
async function readJson(request, limit) {
  if (Number(request.headers.get("Content-Length") || 0) > limit || !request.body) return null;
  // Read in chunks and stop at the limit: a body without Content-Length must not be taken in whole first.
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const raw = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    raw.set(chunk, at);
    at += chunk.byteLength;
  }
  try {
    const value = JSON.parse(new TextDecoder().decode(raw));
    return value && typeof value === "object" && !Array.isArray(value) ? value : null;
  } catch {
    return null;
  }
}

async function allowed(limiter, key) {
  return !limiter || (await limiter.limit({ key })).success;
}

function slowDown(reply) {
  return reply(429, { error: "slow down", retry_after: 10 });
}

function encode(content) {
  return new TextEncoder().encode(JSON.stringify(content));
}

function text(value, chars) {
  return typeof value === "string" ? [...value].slice(0, chars).join("") : "";
}

function corsHeaders(origin, env) {
  const allowedOrigins = (env.ALLOWED_ORIGINS || "").split(",").map(o => o.trim()).filter(Boolean);
  const ok = allowedOrigins.includes(origin) || /^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(origin);
  return ok
    ? {
        "Access-Control-Allow-Origin": origin,
        "Access-Control-Allow-Methods": "GET, POST",
        "Access-Control-Allow-Headers": "Content-Type",
        "Access-Control-Max-Age": "86400",
        Vary: "Origin",
      }
    : { Vary: "Origin" };
}

async function sha256(value) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return [...new Uint8Array(digest)].map(b => b.toString(16).padStart(2, "0")).join("");
}
