# MapleLens push relay

A Cloudflare Worker between MapleLens and the player's phone app. MapleLens (the web page, later the Windows app)
posts notifications with the player's pairing key, and the relay pushes them, encrypted, to every phone paired with
that key (Web Push). MapleLens also uploads the store's state (sales, chat, visitors, mesos) sealed with a key only the
paired devices can make, and the phone app reads it back. The phone side is `docs/app/phone/`; the senders are
`crates/core/src/notify/phone.rs` (notifications) and `docs/app/storebook.js` (the store's state).

| Route | Body | Does |
| --- | --- | --- |
| `GET /key` | | The VAPID public key, for `PushManager.subscribe` |
| `POST /pair` | `{key, subscription}` | A phone joins the pairing (at most 5; the phones already in it get a "new phone" alert) |
| `POST /unpair` | `{key, endpoint?}` | One phone leaves; without `endpoint` the whole pairing is deleted |
| `POST /notify` | `{key, title, body, icon?, tag?}` | Push to every paired phone |
| `POST /status` | `{key}` | `{phones}`: how many are paired |
| `POST /state/put` | `{key, blob}` | MapleLens uploads the sealed store state; answers `{phones}` |
| `POST /state/ping` | `{key}` | MapleLens checks in (each minute); answers `{phones}`, so it notices a phone leaving |
| `POST /state/get` | `{key}` | The phone app reads `{blob, at, seen, phones}` (`seen`: MapleLens's last check-in) |

Removing on the computer (`/unpair` without `endpoint`) pushes a "disconnected" notification to every phone, whose app
clears itself, and leaves a tombstone: that key answers 410 on every route from then on.

No dependencies: VAPID (RFC 8292) and message encryption (RFC 8291) are written on WebCrypto in `src/webpush.js`.
Storage: KV holds pairings (secret hash + push subscriptions); a Durable Object per pairing (`src/store.js`, SQLite
backed) holds the sealed store state, a copy of the hash and phone count (so the frequent state routes need no KV
read), and the tombstone of a removed pairing (KV is eventually consistent; the object is not).

## Deploy

Needs Node 20+ and a Cloudflare account (`npx wrangler login`). From this folder:

```bash
npm test
```

```bash
npx wrangler deploy
```

Run `npm test` before every deploy. For a **new** account (from scratch), do this once before the first deploy:

1. Create the KV namespace and put its `id` in `wrangler.toml` under `[[kv_namespaces]]`:
   ```bash
   npx wrangler kv namespace create PAIRS
   ```
2. Make the VAPID key pair (prints two lines, `VAPID_PRIVATE_JWK=...` and `VAPID_PUBLIC=...`):
   ```bash
   node keys.mjs
   ```
3. Store both as secrets; paste each value when asked (never commit them):
   ```bash
   npx wrangler secret put VAPID_PRIVATE_JWK
   ```
   ```bash
   npx wrangler secret put VAPID_PUBLIC
   ```
4. Deploy (above). The first deploy also creates the `StoreState` Durable Object class (the `[[migrations]]` entry). The relay's address is printed; it must match `RELAY_URL` in `docs/app/phone/relay.js`,
   `docs/app/phone/sw.js` and `crates/core/src/notify/phone.rs`.

A new VAPID key pair makes every phone pair again (their subscriptions are tied to the old public key).
`ALLOWED_ORIGINS` (in `wrangler.toml`) lists the sites whose pages may call the relay; `http://localhost` is always
allowed, for development. A test copy of the site on another origin needs its origin added there.

Always deploy with this folder's config (`-c wrangler.toml` if another `wrangler.*` file could be found above it).

Costs: the free Workers plan covers 100,000 requests and 1,000 KV writes a day, and SQLite Durable Objects 100,000
requests a day. Only pairing, unpairing and forgetting an uninstalled phone write KV (an unchanged phone re-pairing
does not). A watched store costs at most 12 state uploads and 1 check-in a minute, and only while a phone is paired
and something changed; an open phone app reads every 15 s. Watch the dashboard's Workers, KV and Durable Objects
metrics; the paid plan ($5 a month) lifts every limit far above this.

## Rate limits

Cloudflare's rate limiting binding (counted per Cloudflare location, 60 s windows), in `wrangler.toml`:

| Limit | Key | Allows | Why |
| --- | --- | --- | --- |
| `CLIENT_LIMIT` | caller's IP | 60 POSTs a minute | Checked before the body is read: junk can't buy KV reads |
| `PAIR_LIMIT` | caller's IP | 5 pair/unpair a minute | They write KV, the scarce resource |
| `NOTIFY_LIMIT` | pairing id (+ route) | 20 `/notify` and 20 `/status` a minute | A runaway or leaked sender can't flood a phone |
| `STATE_LIMIT` | pairing id (+ route) | 60 of each `/state/*` a minute | Uploads every 5 s at most, reads every 15 s per phone |

A real store sends far less: MapleLens batches a burst of sales into one message, and the page polls `/status` every
4 s only while its QR code is up (at most 2 minutes). Over a limit the answer is 429 with `retry_after`.

## Security

What it protects, and how (each is a test in `test/relay.test.mjs`):

- **Only the key holder can send, read the phone count, unpair or join.** The key is `<id>.<secret>`, 96 + 128
  random bits made by MapleLens (`newKey` in `docs/app/phone/relay.js`). KV keeps the secret's SHA-256 only, so a
  KV dump cannot send. A wrong secret is answered exactly like a pairing that does not exist (no "exists" oracle).
- **The relay cannot read the store.** The state (players' names, chat) is sealed in the browser with AES-GCM under
  a key derived from the pairing secret (HKDF), which the relay never has; it stores and serves ciphertext.
- **Remove means removed.** A removed key is refused on every route at once (410), even by a phone still holding it
  that re-pairs on start, and even where a stale KV copy is read in the minute after (the tombstone is kept for
  good); its phones are told, and their apps clear themselves. A phone that leaves on its own is seen by MapleLens's next check-in.
- **Nobody else reads the messages.** Each push is encrypted for the phone's own keys (aes128gcm); the push service
  (Apple, Google, Mozilla, Microsoft) sees ciphertext. The relay keeps no message: nothing is written to KV or logged.
- **A leaked QR code is noticed and can be revoked.** The code is the key. MapleLens shows it for 2 minutes at most
  and hides it as soon as a phone pairs (a streamed screen is public). Every phone that joins a pairing triggers an
  alert on the phones already in it, and **Remove** in MapleLens deletes the pairing, so a new QR code is a new key.
- **The relay can't be pointed at other servers.** Subscriptions must be `https` URLs on the browsers' push hosts (no
  port, no credentials, no lookalike hosts), and redirects are not followed. Icons are accepted from the item
  picture host only (`maplestory.io/api/`): any other icon URL would tell its server the phone's IP address.
- **Other websites can't use it from their visitors' browsers.** CORS answers only `ALLOWED_ORIGINS` and localhost,
  and a request with any other `Origin` is refused. Non-browser callers still need a key; the key is the credential.
- **Only the relay speaks for the relay.** The tags it sends itself (`unpaired`, which makes a phone forget its key,
  and `new-phone`) are stripped from anyone else's message, so a leaked key cannot fake a removal.
- **A stale copy can't hijack.** Once the pairing's Durable Object knows the secret's hash, a different secret is
  refused even where KV has not caught up yet.
- **The phone page keeps its key to itself.** It sends no referrer, and its Content Security Policy only lets it
  run scripts from itself and jsDelivr, load images from maplestory.io and talk to the relay. The QR decoder from
  jsDelivr is pinned by its SRI hash.
- **Abuse stays bounded.** Bodies over 8 KB (150 KB for a store state) are refused, read in chunks and cut off at the limit, text fields are cut (title 120, body 600 characters),
  and a pairing holds 5 phones at most. The rate limits are above.

What it does not protect: anyone holding a key (a QR code photographed from a stream, a shared screenshot) can send
messages to those phones and pair their own until the player presses Remove. That is why the code hides itself and
every new phone is announced.
