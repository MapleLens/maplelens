// Makes the relay's VAPID key pair: the private key as a JWK (the Worker secret VAPID_PRIVATE_JWK) and the public key
// as base64url raw bytes (VAPID_PUBLIC). Run once; a new pair unsubscribes nobody but makes every phone pair again.
//   node keys.mjs
import { webcrypto as crypto } from "node:crypto";

const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
const jwk = await crypto.subtle.exportKey("jwk", pair.privateKey);
const raw = Buffer.from(await crypto.subtle.exportKey("raw", pair.publicKey)).toString("base64url");
console.log(`VAPID_PRIVATE_JWK=${JSON.stringify(jwk)}`);
console.log(`VAPID_PUBLIC=${raw}`);
