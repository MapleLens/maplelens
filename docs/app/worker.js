// The MapleLens engine (Rust compiled to WebAssembly) in a worker, so reading frames never stutters the page.
//
// Messages in:  {type: "init", fonts, glyphs, cardScale} · {type: "frame", bitmap, inset, scale} ·
//               {type: "ocr-answer", id, text} · {type: "ocr-enabled", enabled} · {type: "shop", enabled} ·
//               {type: "notify", webhook, sales, store} · {type: "notify-test", webhook} · {type: "catalog", id, json}
// Messages out: {type: "ready", version, items} · {type: "state", state, fps, ms} · {type: "card", which, version,
//               width, height, pixels} · {type: "ocr-job", id, font, width, height, pixels} · {type: "glyphs", font,
//               json} · {type: "idle"} (ready for the next frame) · {type: "notify-status", text} ·
//               {type: "notify-test-result", ok, text} · {type: "catalog-loaded", id, items | error} ·
//               {type: "error", message}

import init, { Engine } from "./pkg/maplelens_web.js?v=737f62b708b3";

/** One rank lookup every two seconds stays under classicrank.com's 30 requests a minute. */
const RANK_GAP_MS = 2000;

let engine = null;
let canvas = null;
let context = null;
let lastCard = 0;
let lastRank = 0;
let lastRankFetch = 0;
const askedOcr = new Set();
/** Webhook messages waiting to go out, sent one at a time: a webhook takes about five messages in two seconds. */
const outbox = [];
let sending = false;
const SEND_GAP_MS = 600;
const SEND_RETRIES = 3;
const SEND_MAX_WAIT_MS = 30000;
const frameTimes = [];

const post = (message, transfer = []) => self.postMessage(message, transfer);

async function start({ fonts, glyphs, cardScale, ocr }) {
  // Named explicitly so scripts/stamp-web.ps1 can version it together with the glue.
  await init({ module_or_path: new URL("./pkg/maplelens_web_bg.wasm?v=737f62b708b3", import.meta.url) });
  const [regular, bold] = await Promise.all(fonts.map(url => fetch(url).then(r => r.arrayBuffer())));
  engine = new Engine(new Uint8Array(regular), new Uint8Array(bold));
  engine.card_scale = cardScale || 1;
  engine.set_ocr(Boolean(ocr));
  for (const [font, json] of Object.entries(glyphs || {})) {
    try { engine.learn_glyphs(font, json); } catch { /* an old or damaged copy: the bundled letters still work */ }
  }
  post({ type: "ready", version: Engine.version(), items: engine.item_count() });
}

/** POST one request to a webhook; resolves to {ok, text}: in words for the player, never with the URL (its token
 *  is a secret). A refusal for going too fast is retried after the wait the service asks for. */
async function deliver({ url, body }) {
  for (let attempt = 0; attempt <= SEND_RETRIES; attempt++) {
    let response;
    try {
      response = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body });
    } catch {
      return { ok: false, text: "Discord could not be reached" };
    }
    if (response.ok) return { ok: true, text: "Sent" };
    if (response.status === 429 && attempt < SEND_RETRIES) {
      const info = await response.json().catch(() => ({}));
      const wait = Math.min(SEND_MAX_WAIT_MS, Math.max(100, (Number(info.retry_after) || 1) * 1000));
      await new Promise(resolve => setTimeout(resolve, wait));
      continue;
    }
    if (response.status === 401 || response.status === 404) return { ok: false, text: "Discord says that webhook does not exist (deleted?)" };
    return { ok: false, text: `Discord answered HTTP ${response.status}` };
  }
  return { ok: false, text: "Discord keeps refusing for going too fast" };
}

async function drainOutbox() {
  if (sending) return;
  sending = true;
  try {
    while (outbox.length) {
      const result = await deliver(outbox.shift());
      post({ type: "notify-status", ok: result.ok, text: result.ok ? "Last message sent to Discord" : result.text });
      await new Promise(resolve => setTimeout(resolve, SEND_GAP_MS));
    }
  } finally {
    sending = false;
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
  post({ type: "card", which, version, width, height, pixels }, [pixels.buffer]);
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
  post({ type: "state", state, fps, ms });
  if (state.card && state.card !== lastCard) sendCard("card", (lastCard = state.card));
  if (state.rank && state.rank !== lastRank) sendCard("rank", (lastRank = state.rank));
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
      } catch (error) {
        post({ type: "catalog-loaded", id: data.id, error: String(error?.message || error) });
      }
    }
    else if (data.type === "shop" && engine) engine.set_shop(data.enabled);
    else if (data.type === "notify" && engine) engine.set_notify(data.webhook, data.sales, data.store);
    else if (data.type === "notify-test" && engine) {
      const request = engine.notify_test_request(data.webhook, Date.now() / 1000);
      if (!request) post({ type: "notify-test-result", ok: false, text: "Paste a Discord webhook URL first" });
      else deliver(JSON.parse(request)).then(r => post({ type: "notify-test-result", ok: r.ok, text: r.ok ? "Sent: check your Discord channel" : `Not sent: ${r.text}` }));
    }
  } catch (error) {
    post({ type: "error", message: String(error?.message || error) });
  } finally {
    if (data.type === "frame") post({ type: "idle" });
  }
};
