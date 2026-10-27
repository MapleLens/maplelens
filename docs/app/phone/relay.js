// The phone notification relay (relay/push in the repository): shared by the phone page and the MapleLens page that
// shows the pairing QR code. The Rust side's copy is `notify::phone::RELAY_URL`; sw.js repeats these too.

export const RELAY_URL = "https://maplelens-push.prices-proxy.workers.dev";
/** Where the page leaves the pairing key for the service worker (which has no localStorage). */
export const PAIR_CACHE = "maplelens-phone";
export const PAIR_REQUEST = "pair-key";
/** A pairing key: "<id>.<secret>", both base64url (the relay checks the same shape). */
export const KEY_SHAPE = /^[A-Za-z0-9_-]{16,32}\.[A-Za-z0-9_-]{22,64}$/;

/** A new pairing key: a 96-bit id to find it and a 128-bit secret only the paired devices know. */
export function newKey() {
  const part = bytes => {
    const raw = crypto.getRandomValues(new Uint8Array(bytes));
    return btoa(String.fromCharCode(...raw)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  };
  return `${part(12)}.${part(16)}`;
}

/** The phone page's address for `key`, as the QR code carries it. */
export function phoneUrl(key) {
  return `${new URL("./", import.meta.url).href}?pair=${encodeURIComponent(key)}`;
}

// The store's state (sales, chat, visitors: players' names) goes through the relay sealed with a key only the paired
// devices can make: AES-GCM under HKDF(the pairing secret). The relay stores ciphertext it cannot read.
const encoder = new TextEncoder();
const stateKeys = new Map();

function stateKey(key) {
  if (!stateKeys.has(key)) {
    const secret = encoder.encode(key.split(".")[1]);
    stateKeys.set(
      key,
      crypto.subtle
        .importKey("raw", secret, "HKDF", false, ["deriveKey"])
        .then(base =>
          crypto.subtle.deriveKey(
            { name: "HKDF", hash: "SHA-256", salt: encoder.encode("maplelens/store-state"), info: encoder.encode("v1") },
            base,
            { name: "AES-GCM", length: 256 },
            false,
            ["encrypt", "decrypt"],
          ),
        ),
    );
  }
  return stateKeys.get(key);
}

const toB64url = bytes => {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};
const fromB64url = text => Uint8Array.from(atob(text.replace(/-/g, "+").replace(/_/g, "/")), c => c.charCodeAt(0));

/** `value` as JSON, sealed for the devices holding `key`: base64url(iv || ciphertext). */
export async function seal(key, value) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const sealed = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv }, await stateKey(key), encoder.encode(JSON.stringify(value))));
  const out = new Uint8Array(12 + sealed.length);
  out.set(iv);
  out.set(sealed, 12);
  return toB64url(out);
}

/** What `seal` sealed, or null when it does not open with `key`. */
export async function unseal(key, blob) {
  try {
    const bytes = fromB64url(blob);
    const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: bytes.subarray(0, 12) }, await stateKey(key), bytes.subarray(12));
    return JSON.parse(new TextDecoder().decode(plain));
  } catch {
    return null;
  }
}

/** POST `body` to a relay route; resolves to {ok, status, json}. */
export async function relay(route, body) {
  try {
    const response = await fetch(`${RELAY_URL}/${route}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return { ok: response.ok, status: response.status, json: await response.json().catch(() => ({})) };
  } catch {
    return { ok: false, status: 0, json: {} };
  }
}
