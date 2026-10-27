# MapleLens push relay

A Cloudflare Worker between MapleLens and the player's phone app. MapleLens (the web page, later the Windows app)
posts notifications with the player's pairing key, and the relay pushes them, encrypted, to every phone paired with
that key (Web Push). MapleLens also uploads the store's state (sales, chat, visitors, mesos) sealed with a key only the
paired devices can make, and the phone app reads it back. The phone side is `docs/app/phone/`; the senders are
`crates/core/src/notify/phone.rs` (notifications) and `docs/app/storebook.js` (the store's state).

| Route | Body | Does |
| --- | --- | --- |
| `GET /key` | | The VAPID public key, for `PushManager.subscribe` |
| `POST /pair` | `{key, subscription, replaces?}` | A phone joins the pairing (at most 5: a sixth is refused with 409; the phones already in it get a "new phone" alert, unless it renews its own subscription: `replaces` is the endpoint it had) |
| `POST /unpair` | `{key, endpoint}` | One phone leaves |
| `POST /unpair` | `{key, remove: true}` | MapleLens removes the pairing (a request with neither is refused) |
| `POST /notify` | `{key, title, body, icon?, tag?}` | Push to every paired phone |
| `POST /status` | `{key}` | `{phones}`: how many are paired |
| `POST /state/put` | `{key, blob}` | MapleLens uploads the sealed store state; answers `{phones}` |
| `POST /state/ping` | `{key}` | MapleLens checks in (each minute); answers `{phones}`, so it notices a phone leaving |
| `POST /state/get` | `{key}` | The phone app reads `{blob, at, seen, phones}` (`seen`: MapleLens's last check-in) |

Removing on the computer (`/unpair` with `remove: true`) pushes a "disconnected" notification to every phone, whose app
clears itself, and leaves a tombstone: that key answers 410 on every route from then on.

No dependencies: VAPID (RFC 8292) and message encryption (RFC 8291) are written on WebCrypto in `src/webpush.js`.
Storage: a Durable Object per pairing (`src/store.js`, SQLite backed) holds the secret's hash, the phones' push
subscriptions, the sealed store state and, once removed, the tombstone. It is the pairing's only record: an object
runs one request at a time on strongly consistent storage, so two phones pairing at once both stay and a phone that
just paired is seen by the next message from anywhere. (KV, used at first, cached a missing pairing for a minute per
location: Test right after pairing said "no phone is paired".) A pairing unused for 60 days (no phone joining, no
upload or check-in from MapleLens) is deleted by the object's alarm; a removed key's tombstone is kept.

## Deploy

Needs Node 20+ and a Cloudflare account (`npx wrangler login`). From this folder:

```bash
npm test
```

```bash
npx wrangler deploy
```

Run `npm test` before every deploy. For a **new** account (from scratch), do this once before the first deploy:

1. Make the VAPID key pair (prints two lines, `VAPID_PRIVATE_JWK=...` and `VAPID_PUBLIC=...`):
   ```bash
   node keys.mjs
   ```
2. Store both as secrets; paste each value when asked (never commit them):
   ```bash
   npx wrangler secret put VAPID_PRIVATE_JWK
   ```
   ```bash
   npx wrangler secret put VAPID_PUBLIC
   ```
3. Deploy (above). The first deploy also creates the `StoreState` Durable Object class (the `[[migrations]]` entry)
   and the custom domain in `routes` (`push.maple.observer`, on the `maple.observer` zone of the same account). That
   address must match `RELAY_URL` in `docs/app/phone/relay.js`, `docs/app/phone/sw.js` and
   `crates/core/src/notify/phone.rs`, and the `connect-src` of `docs/app/index.html` and `docs/app/phone/index.html`.
4. Add the firewall rule (below, "Rate limits").

A new VAPID key pair makes every phone pair again (their subscriptions are tied to the old public key).
`ALLOWED_ORIGINS` (in `wrangler.toml`) lists the sites whose pages may call the relay; `http://localhost` is always
allowed, for development. A test copy of the site on another origin needs its origin added there.

Always deploy with this folder's config (`-c wrangler.toml` if another `wrangler.*` file could be found above it).

Costs: the free Workers plan covers 100,000 requests a day, and SQLite Durable Objects 100,000 requests and 100,000
row writes a day. Pairing, unpairing, forgetting an uninstalled phone and a state upload write (an unchanged phone
re-pairing does not). A watched store costs at most 12 state uploads and 1 check-in a minute, and only while a phone is paired
and something changed; an open phone app reads every 15 s. Watch the dashboard's Workers and Durable Objects
metrics; the paid plan ($5 a month) lifts every limit far above this.

## Rate limits

Cloudflare's rate limiting binding (counted per Cloudflare location, 60 s windows), in `wrangler.toml`:

| Limit | Key | Allows | Why |
| --- | --- | --- | --- |
| `CLIENT_LIMIT` | caller's IP | 60 POSTs a minute | Checked before the body is read: junk can't buy storage reads |
| `PAIR_LIMIT` | caller's IP | 5 pair/unpair a minute | They write storage and push alerts |
| `NOTIFY_LIMIT` | pairing id (+ route) | 20 `/notify` and 20 `/status` a minute | A runaway or leaked sender can't flood a phone |
| `STATE_LIMIT` | pairing id (+ route) | 60 of each `/state/*` a minute | Uploads every 5 s at most, reads every 15 s per phone |

A real store sends far less: MapleLens batches a burst of sales into one message, and the page polls `/status` every
4 s only while its QR code is up (at most 2 minutes). Over a limit the answer is 429 with `retry_after`.

These are a brake, not a wall. Cloudflare's binding counts per data centre and settles its counts after the fact, so
a fast burst gets through before it bites: in the pentest, 70 back-to-back requests from one address were limited
1, 10 and 0 times in three runs (the last with a forged `CF-Connecting-IP`, which made no difference). Nothing
depends on them for access: every route needs a key of 224 random bits. They keep a runaway sender or a careless
script from costing much.

The hard ceiling is the zone's firewall, before the Worker runs (so a blocked request costs no Worker or Durable
Object request): one rate-limiting rule on `maple.observer` (the free plan's one), in the dashboard under Security →
WAF → Rate limiting rules, or by the Rulesets API's `http_ratelimit` phase:

| Field | Value |
| --- | --- |
| Match | URI path wildcard `/*` (the free plan matches on the path only; the relay is all the zone serves) |
| Counted by | IP address (and data centre) |
| Limit | 30 requests per 10 seconds |
| Action | Block, for 10 seconds |

A store's page sends at most one request every 4 s, a phone app one every 15 s, so 30 in 10 s is only ever a flood.
Tested: 60 requests in a row from one address, the first 30 answered and the rest blocked (429, by Cloudflare,
before the Worker), and answers again 25 s later. A page on the zone other than the relay would share this limit.
`workers_dev = false` in `wrangler.toml` keeps the Worker off its `workers.dev` address, which no firewall rule
covers. This is this Worker only: the price relay (`maplelens-prices.prices-proxy.workers.dev`, outside
this repo) keeps its `workers.dev` address, which the web app and the Windows app read prices from.

## Security

What it protects, and how (each is a test in `test/relay.test.mjs`):

- **Only the key holder can send, read the phone count, unpair or join.** The key is `<id>.<secret>`, 96 + 128
  random bits made by MapleLens (`newKey` in `docs/app/phone/relay.js`). Storage keeps the secret's SHA-256 only,
  so a dump of it cannot send. A wrong secret is answered exactly like a pairing that does not exist (no "exists" oracle).
- **The relay cannot read the store.** The state (players' names, chat) is sealed in the browser with AES-GCM under
  a key derived from the pairing secret (HKDF), which the relay never has; it stores and serves ciphertext.
- **Remove means removed.** A removed key is refused on every route at once (410), even by a phone still holding it
  that re-pairs on start (the tombstone is kept for good); its phones are told, and their apps clear themselves.
  Removing must be asked for (`remove: true`): a phone leaving without naming itself is refused, so it cannot cut
  off the other phones by accident. MapleLens forgets its key only once the relay has answered. A phone that leaves on its own is seen by MapleLens's next check-in.
- **Nobody else reads the messages.** Each push is encrypted for the phone's own keys (aes128gcm); the push service
  (Apple, Google, Mozilla, Microsoft) sees ciphertext. The relay keeps no message: nothing of it is stored or logged.
- **A leaked QR code is noticed and can be revoked.** The code is the key. MapleLens shows it for 2 minutes at most
  and hides it as soon as a phone pairs (a streamed screen is public). Every phone that joins a pairing triggers an
  alert on the phones already in it, and **Remove** in MapleLens deletes the pairing, so a new QR code is a new key.
- **The relay can't be pointed at other servers.** Subscriptions must be `https` URLs on the browsers' push hosts (no
  port, no credentials, no lookalike hosts, and only in the URL standard's own form, so a backslash or an odd
  encoding that two parsers read differently is refused), and redirects are not followed. Icons are accepted from the item
  picture host only (`maplestory.io/api/`): any other icon URL would tell its server the phone's IP address.
- **Other websites can't use it from their visitors' browsers.** CORS answers only `ALLOWED_ORIGINS` and localhost,
  and a request with any other `Origin` is refused. Non-browser callers still need a key; the key is the credential.
- **Only the relay speaks for the relay.** The tags it sends itself (`unpaired`, which makes a phone forget its key,
  and `new-phone`) are stripped from anyone else's message, so a leaked key cannot fake a removal.
- **An id is its first secret's.** Once a secret has paired with an id, a different secret is refused, even after
  the last phone left.
- **The phone page keeps its key to itself.** It sends no referrer, and its Content Security Policy only lets it
  run scripts from itself (the QR decoder is vendored: `scripts/vendor.mjs`), load images from maplestory.io and
  talk to the relay. The web app and the website follow the same rule (see AGENTS.md, "The site's origin holds
  secrets").
- **Accepted:** a request for an id that has never been used takes longer (its Durable Object is created) than one
  for an existing id. It tells nothing useful: ids are 96 random bits and only ever appear next to their secret.
  Every request with a well-formed key reaches a Durable Object, so someone sending many with random ids uses up the
  free plan's daily object requests (100,000) for everyone, as fast as the rate limits let them; the firewall rule
  above caps one address at 3 requests a second, and the paid plan lifts the quota if many addresses join in. Ids claimed and never used are deleted after 60 days.
- **Abuse stays bounded.** Bodies over 8 KB (150 KB for a store state) are refused, read in chunks and cut off at the limit, text fields are cut (title 120, body 600 characters),
  and a pairing holds 5 phones at most: a sixth is refused rather than one dropped, which would leave the owner's
  phone silent while it looked fine. The rate limits are above.
- **A phone is not taken over by a link.** A phone already paired that opens another pairing's code asks before
  switching, joins the new pairing before leaving the old one, and acts on a removal only for the pairing it holds
  (the "disconnected" push names it).

What it does not protect: anyone holding a key (a QR code photographed from a stream, a shared screenshot) can send
messages to those phones, pair their own, upload a store state of their making (the phones and MapleLens hold the
same key, so a sealed state proves it came from a key holder, not from MapleLens) and remove the pairing, until the
player presses Remove. That is why the code hides itself and
every new phone is announced.
