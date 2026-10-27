// Web Push with WebCrypto only: VAPID (RFC 8292) to say who sends, aes128gcm (RFC 8291) so the push service never
// reads the message. No library: these two are all a sender needs, and fewer dependencies is less to trust.

const encoder = new TextEncoder();
/** A VAPID token is good for up to 24 h; 12 h leaves room for clock skew at the push service. */
const VAPID_LIFETIME_S = 12 * 3600;
/** The record size field: one record holds the whole message (the payload is far under it). */
const RECORD_SIZE = 4096;

/** Signed tokens per push-service origin, reused until near expiry: signing is the costly part. */
const tokens = new Map();
let signingKey;

export async function vapidHeader(env, audience) {
  const now = Math.floor(Date.now() / 1000);
  const cached = tokens.get(audience);
  if (cached && cached.exp - now > 3600) return cached.header;
  signingKey ??= await crypto.subtle.importKey(
    "jwk",
    JSON.parse(env.VAPID_PRIVATE_JWK),
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign"],
  );
  const exp = now + VAPID_LIFETIME_S;
  const head = b64url(encoder.encode(JSON.stringify({ typ: "JWT", alg: "ES256" })));
  const claims = b64url(encoder.encode(JSON.stringify({ aud: audience, exp, sub: env.VAPID_SUBJECT })));
  // WebCrypto's ECDSA signature is r || s, which is what JWS ES256 wants.
  const signature = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, signingKey, encoder.encode(`${head}.${claims}`));
  const header = `vapid t=${head}.${claims}.${b64url(new Uint8Array(signature))}, k=${env.VAPID_PUBLIC}`;
  tokens.set(audience, { header, exp });
  return header;
}

/** `payload` encrypted for the subscription's keys, as one aes128gcm record with its header. */
export async function encryptPayload(keys, payload) {
  const phonePublic = fromB64url(keys.p256dh);
  const authSecret = fromB64url(keys.auth);
  const ours = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]);
  const ourPublic = new Uint8Array(await crypto.subtle.exportKey("raw", ours.publicKey));
  const phoneKey = await crypto.subtle.importKey("raw", phonePublic, { name: "ECDH", namedCurve: "P-256" }, false, []);
  const shared = new Uint8Array(await crypto.subtle.deriveBits({ name: "ECDH", public: phoneKey }, ours.privateKey, 256));

  const keyInfo = concat(encoder.encode("WebPush: info\0"), phonePublic, ourPublic);
  const ikm = await hkdf(authSecret, shared, keyInfo, 32);
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const cek = await hkdf(salt, ikm, encoder.encode("Content-Encoding: aes128gcm\0"), 16);
  const nonce = await hkdf(salt, ikm, encoder.encode("Content-Encoding: nonce\0"), 12);

  const aes = await crypto.subtle.importKey("raw", cek, "AES-GCM", false, ["encrypt"]);
  // 0x02: the last (and only) record's delimiter.
  const sealed = await crypto.subtle.encrypt({ name: "AES-GCM", iv: nonce }, aes, concat(payload, new Uint8Array([2])));

  const header = new Uint8Array(16 + 4 + 1 + ourPublic.length);
  header.set(salt, 0);
  new DataView(header.buffer).setUint32(16, RECORD_SIZE);
  header[20] = ourPublic.length;
  header.set(ourPublic, 21);
  return concat(header, new Uint8Array(sealed));
}

async function hkdf(salt, ikm, info, length) {
  const key = await crypto.subtle.importKey("raw", ikm, "HKDF", false, ["deriveBits"]);
  return new Uint8Array(await crypto.subtle.deriveBits({ name: "HKDF", hash: "SHA-256", salt, info }, key, length * 8));
}

function concat(...parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

export function b64url(bytes) {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromB64url(text) {
  const binary = atob(text.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((text.length + 3) % 4));
  return Uint8Array.from(binary, c => c.charCodeAt(0));
}
