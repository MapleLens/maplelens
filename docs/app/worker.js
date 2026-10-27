// The MapleLens engine (Rust compiled to WebAssembly) in a worker, so reading frames never stutters the page.
//
// Messages in:  {type: "init", fonts, glyphs, cardScale} · {type: "frame", bitmap, inset, scale} ·
//               {type: "ocr-answer", id, text} · {type: "ocr-enabled", enabled} · {type: "shop", enabled} ·
//               {type: "notify", webhook, telegramWebhook, phone, sales, store} ·
//               {type: "notify-test", service, webhook, telegramWebhook, phone} · {type: "phone-qr", url} ·
//               {type: "catalog", id, json, unpictured (item names whose sales have no picture)}
// Messages out: {type: "ready", version, items} · {type: "state", state, fps, ms} · {type: "card", which, version,
//               width, height, pixels, about (the item card's {name, price, trend}; the rank panel's {read, status,
//               rank})} · {type: "ocr-job", id, font, width, height, pixels} · {type: "glyphs", font,
//               json} · {type: "idle"} (ready for the next frame) · {type: "notify-status", text} ·
//               {type: "notify-test-result", ok, text, telegramWebhook?} · {type: "phone-qr", modules} ·
//               {type: "picture", name, url} (a sold item's picture, once found) · {type: "catalog-loaded", id, items | error} ·
//               {type: "error", message}

import init, { Engine } from "./pkg/maplelens_web.js?v=347aacee3ee9";

/** One rank lookup every two seconds stays under classicrank.com's 30 requests a minute. */
const RANK_GAP_MS = 2000;

let engine = null;
let canvas = null;
let context = null;
let lastCard = 0;
let lastRank = 0;
let lastRankFetch = 0;
const askedOcr = new Set();
/** Messages waiting to go out, sent one at a time: a webhook takes about five messages in two seconds, a Telegram
 *  chat about one a second. */
const outbox = [];
let sending = false;
const SEND_GAP_MS = 600;
const SEND_RETRIES = 3;
const SEND_MAX_WAIT_MS = 30000;
const frameTimes = [];

const post = (message, transfer = []) => self.postMessage(message, transfer);

async function start({ fonts, glyphs, cardScale, ocr }) {
  // Named explicitly so scripts/stamp-web.ps1 can version it together with the glue.
  await init({ module_or_path: new URL("./pkg/maplelens_web_bg.wasm?v=347aacee3ee9", import.meta.url) });
  const [regular, bold] = await Promise.all(fonts.map(url => fetch(url).then(r => r.arrayBuffer())));
  engine = new Engine(new Uint8Array(regular), new Uint8Array(bold));
  engine.card_scale = cardScale || 1;
  engine.set_ocr(Boolean(ocr));
  for (const [font, json] of Object.entries(glyphs || {})) {
    try { engine.learn_glyphs(font, json); } catch { /* an old or damaged copy: the bundled letters still work */ }
  }
  post({ type: "ready", version: Engine.version(), items: engine.item_count() });
}

/** POST one request to a service; resolves to {ok, text, answer}: text in words for the player, never with the URL
 *  (its token is a secret). A refusal for going too fast is retried after the wait the service asks for (Discord
 *  says `retry_after`, Telegram `parameters.retry_after`). */
async function deliver({ service = "Discord", url, body }) {
  for (let attempt = 0; attempt <= SEND_RETRIES; attempt++) {
    let response;
    try {
      response = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body });
    } catch {
      return { ok: false, text: `${service} could not be reached` };
    }
    if (response.ok) return { ok: true, text: "Sent", answer: await response.text().catch(() => "") };
    if (response.status === 429 && attempt < SEND_RETRIES) {
      const info = await response.json().catch(() => ({}));
      const seconds = Number(info.retry_after ?? info.parameters?.retry_after) || 1;
      await new Promise(resolve => setTimeout(resolve, Math.min(SEND_MAX_WAIT_MS, Math.max(100, seconds * 1000))));
      continue;
    }
    return { ok: false, text: Engine.notify_refusal(service, response.status) };
  }
  return { ok: false, text: `${service} keeps refusing for going too fast` };
}

/** `request` sent, or its `fallback` if the service refuses it (a picture it could not fetch: the same text alone). */
async function deliverOrFallback(request) {
  const result = await deliver(request);
  return result.ok || !request.fallback ? result : deliver({ service: request.service, ...request.fallback });
}

/** A test message to one service; a Telegram URL without its chat is first completed by asking the bot who wrote to
 *  it, and the whole URL comes back with the answer for the page to keep. */
async function testNotify({ service, webhook, telegramWebhook, phone }) {
  const now = Date.now() / 1000;
  if (service === "phone") {
    const request = engine.phone_test_request(phone, now);
    if (!request) return { ok: false, text: "Connect a phone first" };
    const r = await deliver(JSON.parse(request));
    return { ok: r.ok, text: r.ok ? "Sent: check your phone" : `Not sent: ${r.text}` };
  }
  if (service !== "telegram") {
    const request = engine.notify_test_request(webhook, now);
    if (!request) return { ok: false, text: "Paste a Discord webhook URL first" };
    const r = await deliverOrFallback(JSON.parse(request));
    return { ok: r.ok, text: r.ok ? "Sent: check your Discord channel" : `Not sent: ${r.text}` };
  }
  let url = telegramWebhook;
  let found;
  if (!engine.telegram_test_request(url, now)) {
    const link = Engine.telegram_link_request(url);
    if (!link) return { ok: false, text: "Paste your bot's URL or token first" };
    const r = await deliver({ service: "Telegram", ...JSON.parse(link) });
    if (!r.ok) return { ok: false, text: `Not linked: ${r.text}` };
    found = url = Engine.telegram_linked(url, r.answer);
    if (!url) return { ok: false, text: "No message to the bot yet: send it /start in Telegram, then Test again" };
  }
  const r = await deliverOrFallback(JSON.parse(engine.telegram_test_request(url, now)));
  return { ok: r.ok, text: r.ok ? "Sent: check your Telegram chat" : `Not sent: ${r.text}`, telegramWebhook: found };
}

async function drainOutbox() {
  if (sending) return;
  sending = true;
  try {
    while (outbox.length) {
      const request = outbox.shift();
      const result = await deliverOrFallback(request);
      post({ type: "notify-status", service: request.service, ok: result.ok, text: result.ok ? `Last message sent to ${request.service}` : result.text });
      await new Promise(resolve => setTimeout(resolve, SEND_GAP_MS));
    }
  } finally {
    sending = false;
  }
}

/** The sold items' pictures, looked up by name on maplestory.io before the sales go out (the engine waits a few
 *  seconds for them; a failed lookup is not handed in). */
function lookUpPictures() {
  for (const { name, url } of JSON.parse(engine.take_picture_lookups())) {
    fetch(url, { headers: { Accept: "application/json" } })
      .then(response => response.ok ? response.text() : null)
      .then(answer => {
        if (answer === null) return;
        engine.picture_found(name, answer);
        // The page's store log and the phone app show it beside the sale too.
        const picture = engine.picture_url(name);
        if (picture) post({ type: "picture", name, url: picture });
      })
      .catch(() => {});
  }
}

function collectRequests() {
  const requests = JSON.parse(engine.take_requests());
  if (!requests.length) return;
  outbox.push(...requests);
  drainOutbox();
}

function sendCard(which, version) {
  const [width, height, pixels] = which === "card"
    ? [engine.card_width(), engine.card_height(), engine.card_pixels()]
    : [engine.rank_width(), engine.rank_height(), engine.rank_pixels()];
  const about = JSON.parse(which === "card" ? engine.card_about() : engine.rank_about());
  post({ type: "card", which, version, width, height, pixels, about }, [pixels.buffer]);
}

async function lookUpRanks() {
  if (Date.now() - lastRankFetch < RANK_GAP_MS) return;
  const request = engine.next_rank_request();
  if (!request) return;
  lastRankFetch = Date.now();
  const { name, url } = JSON.parse(request);
  try {
    const response = await fetch(url, { headers: { Accept: "application/json" } });
    const retry = Number(response.headers.get("Retry-After")) || 0;
    engine.rank_response(name, response.status, await response.text(), retry, performance.now() / 1000);
  } catch {
    engine.rank_response(name, 0, "", 0, performance.now() / 1000);
  }
}

function frame({ bitmap, inset, scale }) {
  if (!canvas || canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
    canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    context = canvas.getContext("2d", { willReadFrequently: true });
  }
  context.drawImage(bitmap, 0, 0);
  bitmap.close();
  const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
  const started = performance.now();
  const state = JSON.parse(engine.process(pixels, canvas.width, canvas.height, inset, scale, performance.now() / 1000, Date.now() / 1000));
  const ms = performance.now() - started;
  frameTimes.push(performance.now());
  while (frameTimes.length > 20) frameTimes.shift();
  const fps = frameTimes.length > 1 ? (frameTimes.length - 1) * 1000 / (frameTimes.at(-1) - frameTimes[0]) : 0;
  // A new picture goes before the state that shows it, so the page never shows a state with the last item's picture.
  if (state.card && state.card !== lastCard) sendCard("card", (lastCard = state.card));
  if (state.rank && state.rank !== lastRank) sendCard("rank", (lastRank = state.rank));
  post({ type: "state", state, fps, ms });
  if (state.ocr > 0) {
    const jobs = JSON.parse(engine.ocr_jobs()).filter(job => !askedOcr.has(job.id));
    for (const job of jobs) {
      askedOcr.add(job.id);
      const pixels = engine.ocr_job_pixels(job.id);
      post({ type: "ocr-job", ...job, pixels }, [pixels.buffer]);
    }
  }
  for (const font of ["title", "name"]) {
    const json = engine.learnt_glyphs(font);
    if (json) post({ type: "glyphs", font, json });
  }
  lookUpPictures();
  collectRequests();
  lookUpRanks();
}

self.onmessage = async ({ data }) => {
  try {
    if (data.type === "init") await start(data);
    else if (data.type === "frame" && engine) frame(data);
    else if (data.type === "ocr-answer" && engine) engine.ocr_answer(data.id, data.text);
    else if (data.type === "ocr-enabled" && engine) engine.set_ocr(data.enabled);
    else if (data.type === "catalog" && engine) {
      try {
        post({ type: "catalog-loaded", id: data.id, items: engine.load_catalog(data.json) });
        // The store book's sales kept without a picture (from before pictures came by id) get the price list's.
        for (const name of data.unpictured || []) {
          const url = engine.catalog_picture(name);
          if (url) post({ type: "picture", name, url });
        }
      } catch (error) {
        post({ type: "catalog-loaded", id: data.id, error: String(error?.message || error) });
      }
    }
    else if (data.type === "shop" && engine) engine.set_shop(data.enabled);
    else if (data.type === "notify" && engine) {
      engine.set_notify(data.webhook, data.telegramWebhook, data.phone || "", data.sales, data.store);
    }
    else if (data.type === "phone-qr") post({ type: "phone-qr", modules: Engine.phone_qr(data.url) });
    else if (data.type === "notify-test" && engine) testNotify(data).then(r => post({ type: "notify-test-result", ...r }));
  } catch (error) {
    post({ type: "error", message: String(error?.message || error) });
  } finally {
    if (data.type === "frame") post({ type: "idle" });
  }
};
