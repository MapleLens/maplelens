// node --test test/webpush.test.mjs — the relay's crypto, checked by doing the phone's side by hand (RFC 8291 decryption, RFC 8292
// signature check). A real phone is the final check; this catches a wrong byte before it gets there.
import assert from "node:assert/strict";
import test from "node:test";

import { b64url, encryptPayload, fromB64url, vapidHeader } from "../src/webpush.js";

const subtle = crypto.subtle;
const encoder = new TextEncoder();

async function hkdf(salt, ikm, info, length) {
  const key = await subtle.importKey("raw", ikm, "HKDF", false, ["deriveBits"]);
  return new Uint8Array(await subtle.deriveBits({ name: "HKDF", hash: "SHA-256", salt, info }, key, length * 8));
}

test("a message decrypts with the phone's keys", async () => {
  const phone = await subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]);
  const phonePublic = new Uint8Array(await subtle.exportKey("raw", phone.publicKey));
  const auth = crypto.getRandomValues(new Uint8Array(16));
  const message = encoder.encode(JSON.stringify({ title: "Pet Food ×20 sold", body: "Buyer paid 12,400" }));

  const sealed = await encryptPayload({ p256dh: b64url(phonePublic), auth: b64url(auth) }, message);

  const salt = sealed.slice(0, 16);
  assert.equal(new DataView(sealed.buffer).getUint32(16), 4096);
  const idLength = sealed[20];
  const senderPublic = sealed.slice(21, 21 + idLength);
  const senderKey = await subtle.importKey("raw", senderPublic, { name: "ECDH", namedCurve: "P-256" }, false, []);
  const shared = new Uint8Array(await subtle.deriveBits({ name: "ECDH", public: senderKey }, phone.privateKey, 256));
  const info = new Uint8Array([...encoder.encode("WebPush: info\0"), ...phonePublic, ...senderPublic]);
  const ikm = await hkdf(auth, shared, info, 32);
  const cek = await subtle.importKey("raw", await hkdf(salt, ikm, encoder.encode("Content-Encoding: aes128gcm\0"), 16), "AES-GCM", false, ["decrypt"]);
  const nonce = await hkdf(salt, ikm, encoder.encode("Content-Encoding: nonce\0"), 12);
  const plain = new Uint8Array(await subtle.decrypt({ name: "AES-GCM", iv: nonce }, cek, sealed.slice(21 + idLength)));

  assert.equal(plain.at(-1), 2);
  assert.deepEqual(plain.slice(0, -1), message);
});

test("the VAPID token verifies with the public key and names the push service", async () => {
  const pair = await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const env = {
    VAPID_PRIVATE_JWK: JSON.stringify(await subtle.exportKey("jwk", pair.privateKey)),
    VAPID_PUBLIC: b64url(new Uint8Array(await subtle.exportKey("raw", pair.publicKey))),
    VAPID_SUBJECT: "https://maplelens.github.io/maplelens/",
  };
  const header = await vapidHeader(env, "https://fcm.googleapis.com");
  const [, token, k] = /^vapid t=([^,]+), k=(.+)$/.exec(header);
  assert.equal(k, env.VAPID_PUBLIC);
  const [head, claims, signature] = token.split(".");
  const verifyKey = await subtle.importKey("raw", fromB64url(k), { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
  const ok = await subtle.verify({ name: "ECDSA", hash: "SHA-256" }, verifyKey, fromB64url(signature), encoder.encode(`${head}.${claims}`));
  assert.ok(ok);
  const parsed = JSON.parse(new TextDecoder().decode(fromB64url(claims)));
  assert.equal(parsed.aud, "https://fcm.googleapis.com");
  assert.ok(parsed.exp > Date.now() / 1000 + 11 * 3600);
});
