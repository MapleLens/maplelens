// MapleLens Web: capture the game window (the page offers nothing else; the sample, video and frame-folder sources are
// test hooks started by a URL parameter), feed frames to the engine, show the cards in the page and, on request, in a picture-in-picture window that floats over the game.

import { Lens } from "./lens.js?v=b3be61b47b2d";
import { CardView, floatingSize, openFloating } from "./cards.js?v=b3be61b47b2d";
import { Prices } from "./prices.js?v=b3be61b47b2d";

const $ = id => document.getElementById(id);
const params = new URLSearchParams(location.search);
/** Frames per second sent to the engine: tooltips appear and vanish with the mouse, so ~10 is plenty. */
const FPS = Number(params.get("fps")) || 10;
/** Title bar heights (px) tried when the window mode is "find out by itself": none, then Windows' at 100-150%. */
const INSETS = { full: [0], windowed: [31, 39, 47], auto: [0, 31, 39, 47] };
/** Without any detection for this long, the auto mode starts trying the other title bar heights again. */
const RELOCK_MS = 15000;
const TRY_MS = 1500;
/** Frames the page keeps timings of, for the numbers it shows (and `window.maplelensStats`). */
const STATS_FRAMES = 300;
const SHOP_KEY = "maplelens.web.shop";
/** A Discord webhook URL, as the engine accepts one (it checks again; this is for the buttons). */
const WEBHOOK = /^https:\/\/(discord\.com|discordapp\.com|ptb\.discord\.com|canary\.discord\.com)\/api\/webhooks\/\d+\/[A-Za-z0-9_-]+$/;
/** A Telegram bot's sendMessage URL, with its chat once known; or a token alone (in @BotFather's whole message). The
 *  engine reads them the same way (`telegram::parse_webhook`); this is for the buttons. */
const TELEGRAM_URL = /^https:\/\/api\.telegram\.org\/bot(\d{1,20}:[A-Za-z0-9_-]{30,60})\/sendMessage(?:\?(.*))?$/;
const BOT_TOKEN = /(?:^|[\s`"',.])(\d{1,20}:[A-Za-z0-9_-]{30,60})(?=$|[\s`"',.])/;

/** What was pasted as a Telegram webhook URL in its one form, or "" when it holds no bot. */
function telegramWebhook(text) {
  text = text.trim();
  const url = text.match(TELEGRAM_URL);
  const token = url ? url[1] : text.match(BOT_TOKEN)?.[1];
  if (!token || (!url && text.startsWith("https://api.telegram.org/"))) return "";
  const chatId = url && new URLSearchParams(url[2] || "").get("chat_id");
  if (chatId !== null && chatId !== undefined && !/^-?[1-9]\d{0,18}$/.test(chatId)) return "";
  return `https://api.telegram.org/bot${token}/sendMessage${chatId ? `?chat_id=${chatId}` : ""}`;
}
const LOG_ROWS = 40;
/** A still frame is held this long (and until its text is read, up to the maximum) before the next one. */
const FRAME_HOLD_MS = 4000;
const FRAME_HOLD_MAX_MS = 90000;
/** The engine may take at most a third of the time: after a frame that took `ms`, the next waits twice as long. */
const PACE = 2;
/** With nothing to follow, a quarter-second between looks is fast enough (a tooltip takes longer than that to
 *  appear); while a tooltip or window is being read, the full rate. */
const IDLE_GAP_MS = 250;
const IDLE_AFTER_MS = 1500;
/** Reading nothing for this long while a source runs: say what to check. */
const NOTHING_AFTER_MS = 25000;

let lens = null;
let source = null; // {stop(), next(): Promise<ImageBitmap|null>, scale, label, fullFrame}
let insetIndex = 0;
let lockedUntil = 0;
let triedAt = 0;
let lastSize = [0, 0];
let floating = null; // {window, view}
let pageView = null;
let lastMs = 0;
let lastState = null;
let lastSeen = 0; // when a tooltip or Character Info window was last in the state
let startedAt = 0;
let latest = { card: null, rank: null, title: "", note: "" };
const images = { card: null, rank: null };
const stats = { shopEvents: [], frames: 0, dropped: 0, ms: [], longTasks: 0, started: 0, cards: 0, hovers: 0, titles: {}, mainMs: 0, pumpMs: 0 };
window.maplelensStats = () => summary();

function storage(action) {
  try { return action(window.localStorage); } catch { return undefined; } // private windows may refuse
}

/** What the player set for the shop, Discord and Telegram, kept in this browser. */
const shopSettings = { shop: true, sales: true, store: true, webhook: "", telegramWebhook: "" };

function loadShopSettings() {
  try {
    Object.assign(shopSettings, JSON.parse(storage(s => s.getItem(SHOP_KEY)) || "{}"));
  } catch { /* damaged: the defaults stay */ }
  if (typeof shopSettings.webhook !== "string" || !WEBHOOK.test(shopSettings.webhook)) shopSettings.webhook = "";
  shopSettings.telegramWebhook = typeof shopSettings.telegramWebhook === "string" ? telegramWebhook(shopSettings.telegramWebhook) : "";
  $("shop").checked = shopSettings.shop !== false;
  $("notify-sales").checked = shopSettings.sales !== false;
  $("notify-store").checked = shopSettings.store !== false;
  showWebhook();
  showTelegram();
}

function saveShopSettings() {
  storage(s => s.setItem(SHOP_KEY, JSON.stringify(shopSettings)));
}

/** A saved webhook is shown as dots and its last four characters (a streamer's screen is public); "Change" opens the
 *  field to type another. */
function showWebhook(editing = false) {
  const set = Boolean(shopSettings.webhook);
  $("discord-state").textContent = set ? `•••••••• ${shopSettings.webhook.slice(-4)}` : "Not connected";
  $("discord-state").hidden = editing;
  $("discord").hidden = !editing;
  $("discord").value = "";
  $("discord-change").textContent = editing ? "Cancel" : set ? "Change" : "Add";
  $("discord-test").hidden = !set || editing;
  $("discord-clear").hidden = !set || editing;
  if (editing) $("discord").focus();
}

/** The bot's URL shows the same way (the end of its token); until its chat is known it says so. */
function showTelegram(editing = false) {
  const url = shopSettings.telegramWebhook;
  const set = Boolean(url);
  const token = url.match(TELEGRAM_URL)?.[1] || "";
  $("telegram-state").textContent = !set ? "Not connected" : `•••••••• ${token.slice(-4)}${url.includes("chat_id=") ? "" : " · no chat yet"}`;
  $("telegram-state").hidden = editing;
  $("telegram").hidden = !editing;
  $("telegram").value = "";
  $("telegram-change").textContent = editing ? "Cancel" : set ? "Change" : "Add";
  $("telegram-test").hidden = !set || editing;
  $("telegram-clear").hidden = !set || editing;
  if (editing) $("telegram").focus();
}

/** Pass the settings to the engine. */
function applyShopSettings() {
  if (!lens) return;
  lens.setShop(shopSettings.shop);
  const { webhook, telegramWebhook, sales, store } = shopSettings;
  lens.setNotify({ webhook, telegramWebhook, sales, store });
}

/** Words for a shop event, for the log. */
function describeShopEvent(e, owner) {
  const whose = owner ? `${owner}'s store` : "Your store";
  switch (e.kind) {
    case "sale": return [`Sold ${e.item}${e.quantity > 1 ? ` ×${e.quantity}` : ""} to ${e.buyer}${e.mesos ? ` · +${e.mesos.toLocaleString()} mesos` : ""}`, "sale"];
    case "sale-unseen": return ["Something sold (open the Sold Items window to see what)", "sale"];
    case "sale-ambiguous": return ["Something sold, but the list looks unchanged", "sale"];
    case "entered": return [`${e.name} came in`, ""];
    case "left": return [`${e.name} left`, ""];
    case "chat": return [`${e.text}`, ""];
    case "opened": return [`${whose} was detected open`, ""];
    case "closed": return [`${whose} closed`, ""];
    case "lost-sync": return ["The lists scrolled or were covered: something may be missed", ""];
    default: return null;
  }
}

function logShopEvents(events, owner) {
  const log = $("store-log");
  for (const event of events) {
    const described = describeShopEvent(event, owner);
    if (!described) continue;
    log.querySelector(".none")?.remove();
    const row = document.createElement("li");
    row.className = described[1];
    const time = Object.assign(document.createElement("span"), { className: "t", textContent: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) });
    row.append(time, document.createTextNode(described[0]));
    log.prepend(row);
  }
  while (log.children.length > LOG_ROWS) log.lastChild.remove();
}

/** The Store Permit's status, in the words the Windows app's pill uses (they come from the engine, i.e. from core). */
function shopStatus(state) {
  const status = state.shop?.status;
  return shopSettings.shop && status?.text ? { text: status.text, tone: status.tone } : null;
}

function showShopStatus(state) {
  const store = shopStatus(state);
  setPill("shop-pill", store?.tone === "ok", store?.text || "no store");
  $("store-tag").textContent = !shopSettings.shop ? "off" : store ? store.text : "no store in view";
}

function setPill(id, on, text) {
  $(id).className = `pill ${on ? "on" : ""}`;
  $(id).textContent = text;
}

function summary() {
  const sorted = [...stats.ms].sort((a, b) => a - b);
  const at = q => sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] : 0;
  const seconds = stats.started ? (performance.now() - stats.started) / 1000 : 0;
  return {
    frames: stats.frames, dropped: stats.dropped, cards: stats.cards, hovers: stats.hovers, titles: stats.titles, seconds,
    fps: seconds ? stats.frames / seconds : 0,
    avgMs: sorted.length ? sorted.reduce((a, b) => a + b, 0) / sorted.length : 0,
    p50Ms: at(0.5), p95Ms: at(0.95), maxMs: sorted.at(-1) || 0,
    longTasks: stats.longTasks,
    shopEvents: stats.shopEvents,
    shop: stats.shop, frameSize: lastSize,
    stateMsPerFrame: stats.frames ? stats.mainMs / stats.frames : 0, pumpMsPerFrame: stats.frames ? stats.pumpMs / stats.frames : 0,
    messages: lens?.counts, glyphKB: lens ? lens.bytes / 1024 : 0,
    heapMB: performance.memory ? performance.memory.usedJSHeapSize / 1048576 : null,
  };
}

try {
  new PerformanceObserver(list => { stats.longTasks += list.getEntries().length; }).observe({ entryTypes: ["longtask"] });
} catch { /* not every browser reports long tasks */ }

/** The views that show the cards: the page's panel and, while it is open, the floating window's. */
function views() {
  return [pageView, floating?.view].filter(Boolean);
}

function refreshViews() {
  for (const view of views()) view.update(images, latest);
}

function show(which, image) {
  stats.cards += 1;
  images[which] = image;
  refreshViews();
}

function drawBoxes(state, frameWidth, frameHeight) {
  const stage = $("view");
  const rect = stage.getBoundingClientRect();
  const k = Math.min(rect.width / frameWidth, rect.height / frameHeight);
  const ox = (rect.width - frameWidth * k) / 2;
  const oy = (rect.height - frameHeight * k) / 2;
  const boxes = [];
  if (state.hover?.at) {
    const [x, y] = state.hover.at;
    const [w, h] = state.hover.size;
    boxes.push({ x, y, w, h, label: state.hover.title, kind: "" });
  }
  if (state.info) {
    const [x, y, w, h] = state.info.rect;
    boxes.push({ x, y, w, h, label: state.info.name ? `Character Info: ${state.info.name}` : "Character Info", kind: "info" });
  }
  $("boxes").replaceChildren(...boxes.map(b => {
    const el = document.createElement("div");
    el.className = `box ${b.kind}`;
    Object.assign(el.style, { left: `${ox + b.x * k}px`, top: `${oy + b.y * k}px`, width: `${b.w * k}px`, height: `${b.h * k}px` });
    const label = document.createElement("span");
    label.textContent = b.label;
    el.append(label);
    return el;
  }));
}

function onState(state, fps, ms) {
  const t0 = performance.now();
  try {
    handleState(state, fps, ms);
  } finally {
    stats.mainMs += performance.now() - t0;
  }
}

function handleState(state, fps, ms) {
  lastState = state;
  stats.frames += 1;
  lastMs = ms;
  if (state.hover) {
    stats.hovers += 1;
    stats.titles[state.hover.title] = (stats.titles[state.hover.title] || 0) + 1;
  }
  stats.ms.push(ms);
  if (stats.ms.length > STATS_FRAMES) stats.ms.shift();
  setPill("tooltip-pill", Boolean(state.hover), state.hover ? `tooltip: ${state.hover.title}` : "no tooltip");
  setPill("window-pill", Boolean(state.info), state.info ? `Character Info${state.info.name ? `: ${state.info.name}` : ""}` : "no Character Info");
  showShopStatus(state);
  if (state.shop?.events?.length) {
    logShopEvents(state.shop.events, state.shop.owner);
    stats.shopEvents.push(...state.shop.events);
  }
  stats.shop = { looks: state.shop?.looks, undone: state.shop?.undone, ownerReads: state.shop?.owner_reads };
  const note = `${fps.toFixed(0)} fps · read ${ms.toFixed(0)} ms · UI ${state.scale.toFixed(2)}x${state.ocr ? " · reading new letters" : ""}`;
  $("fps").textContent = note;
  latest = {
    title: state.hover?.title || (state.info?.name ? `${state.info.name}'s ranks` : ""),
    note,
    store: shopStatus(state),
  };
  if (!state.card) images.card = null;
  if (!state.rank) images.rank = null;
  refreshViews();
  drawBoxes(state, ...lastSize);
  // Auto mode: a detection proves the title bar guess right; keep it.
  if (state.hover || state.info) {
    lockedUntil = performance.now() + RELOCK_MS;
    lastSeen = performance.now();
  }
}

function inset() {
  const insets = INSETS.auto;
  const now = performance.now();
  if (now > lockedUntil && now - triedAt > TRY_MS) {
    insetIndex = (insetIndex + 1) % insets.length;
    triedAt = now;
  }
  return insets[insetIndex % insets.length];
}

/** The UI size the player picked, or 0 to let the engine measure it off the game's status bar. */
function manualScale() {
  return Number(params.get("scale")) || 0;
}

/** Longest side (px) of the picture shown in the page: the engine gets the full frame, the player only a preview. */
const PREVIEW_PX = 960;

async function pump() {
  const view = $("view");
  const context = view.getContext("2d");
  let last = 0;
  // Asked by the source before it makes a bitmap of a frame: most of a 60 fps video's frames would be thrown away,
  // and a bitmap of every one of them cost more than reading the ones we keep.
  const due = () => performance.now() - last >= Math.max(performance.now() - lastSeen > IDLE_AFTER_MS ? IDLE_GAP_MS : 1000 / FPS, PACE * lastMs);
  while (source) {
    const bitmap = await source.next(due);
    if (!bitmap || !source) break;
    if (!due()) {
      bitmap.close();
      continue;
    }
    const t0 = performance.now();
    last = t0;
    const k = Math.min(1, PREVIEW_PX / Math.max(bitmap.width, bitmap.height));
    const [w, h] = [Math.round(bitmap.width * k), Math.round(bitmap.height * k)];
    if (view.width !== w || view.height !== h) {
      view.width = w;
      view.height = h;
    }
    context.drawImage(bitmap, 0, 0, w, h);
    lastSize = [bitmap.width, bitmap.height];
    const accepted = lens.frame(bitmap, { inset: source.fullFrame ? 0 : inset(), scale: source.scale || manualScale() });
    if (!accepted) stats.dropped += 1;
    stats.pumpMs += performance.now() - t0;
  }
}

function start(newSource) {
  stop();
  source = newSource;
  startedAt = lastSeen = performance.now();
  Object.assign(stats, { frames: 0, dropped: 0, ms: [], longTasks: 0, cards: 0, hovers: 0, titles: {}, mainMs: 0, pumpMs: 0, started: performance.now() });
  $("start").hidden = true;
  $("view").hidden = false;
  $("stop").hidden = false;
  $("source").textContent = source.label;
  pump();
}

function stop() {
  $("nothing").hidden = true;
  source?.stop();
  source = null;
  $("start").hidden = false;
  $("view").hidden = true;
  $("stop").hidden = true;
  $("boxes").replaceChildren();
  $("source").textContent = "not connected";
  setPill("tooltip-pill", false, "no tooltip");
  setPill("window-pill", false, "no Character Info");
  $("fps").textContent = "";
  latest = { title: "", note: "", store: null };
  images.card = images.rank = null;
  for (const view of views()) view.clear();
  refreshViews();
}

/** Frames of a shared window, one ImageBitmap at a time. */
function windowSource(stream) {
  const track = stream.getVideoTracks()[0];
  track.addEventListener("ended", stop);
  let next;
  if ("MediaStreamTrackProcessor" in window) {
    const reader = new MediaStreamTrackProcessor({ track }).readable.getReader();
    next = async due => {
      for (;;) {
        const { value: frame, done } = await reader.read();
        if (done) return null;
        if (!due()) {
          frame.close();
          continue;
        }
        const bitmap = await createImageBitmap(frame);
        frame.close();
        return bitmap;
      }
    };
  } else {
    const video = Object.assign(document.createElement("video"), { srcObject: stream, muted: true, playsInline: true });
    video.play();
    next = due => new Promise(function wait(resolve) {
      video.requestVideoFrameCallback(() => (due() ? createImageBitmap(video).then(resolve, () => resolve(null)) : wait(resolve)));
    });
  }
  return { label: track.label || "shared window", scale: 0, next, stop: () => stream.getTracks().forEach(t => t.stop()) };
}

/** A recorded clip of the game (a stream's VOD, a screen recording), played in a loop: for trying MapleLens without
 *  the game, and for measuring it. Compression blurs the status bar, so pick the UI size by hand if it reads nothing. */
async function videoSource(src) {
  const url = typeof src === "string" ? src : URL.createObjectURL(src);
  const video = Object.assign(document.createElement("video"), { src: url, muted: true, loop: true, playsInline: true });
  await video.play();
  const next = due => new Promise(function wait(resolve) {
    if (video.ended) return resolve(null);
    video.requestVideoFrameCallback(() => (due() ? createImageBitmap(video).then(resolve, () => resolve(null)) : wait(resolve)));
  });
  const label = typeof src === "string" ? src.split("/").pop() : src.name;
  return { label, scale: 0, fullFrame: true, next, stop: () => { video.pause(); video.removeAttribute("src"); if (typeof src !== "string") URL.revokeObjectURL(url); } };
}

/** A clip played into a media stream, which goes through exactly the code a shared window does (the track
 *  processor): for testing that path without sharing anything. `?capture=clip.mp4`. */
async function capturedVideoSource(url) {
  const video = Object.assign(document.createElement("video"), { src: url, muted: true, loop: true, playsInline: true });
  await video.play();
  const stream = video.captureStream();
  const source = windowSource(stream);
  return { ...source, label: `${url.split("/").pop()} (as a stream)`, stop: () => { source.stop(); video.pause(); } };
}

/** A folder of still frames (`list.json` names them), each held until the engine has looked at it a few times and has
 *  no text left to read: for trying the shop on recorded captures, where a clip's compression would blur the text.
 *  `?frames=/folder/`. */
async function framesSource(base) {
  const names = await (await fetch(`${base}list.json`)).json();
  let index = -1;
  let heldSince = 0;
  let running = true;
  let current = null;
  const hold = () => performance.now() - heldSince;
  const next = async () => {
    for (;;) {
      if (!running) return null;
      // Move on when the frame has been looked at long enough and the text it needs is read.
      if (!current || (hold() > FRAME_HOLD_MS && !lastState?.ocr) || hold() > FRAME_HOLD_MAX_MS) {
        index += 1;
        if (index >= names.length) {
          running = false;
          window.maplelensFramesDone = true;
          return null;
        }
        current = await createImageBitmap(await (await fetch(`${base}${names[index]}`)).blob());
        heldSince = performance.now();
        window.maplelensFrame = names[index];
      }
      await new Promise(resolve => setTimeout(resolve, 250));
      return createImageBitmap(current);
    }
  };
  return { label: `${names.length} frames`, scale: 0, fullFrame: true, next, stop: () => { running = false; } };
}

/** The sample screenshot, replayed: a Character Info window and an Emerald Ore tooltip, drawn at 1x. */
async function demoSource() {
  const image = await createImageBitmap(await (await fetch(new URL("demo-frame.png", import.meta.url))).blob());
  let running = true;
  const next = async () => {
    await new Promise(r => setTimeout(r, 1000 / FPS));
    return running ? createImageBitmap(image) : null;
  };
  return { label: "sample screenshot", scale: 1, fullFrame: true, next, stop: () => { running = false; } };
}

async function share() {
  try {
    const stream = await navigator.mediaDevices.getDisplayMedia({
      video: { displaySurface: "window", frameRate: { ideal: FPS, max: 15 } },
      audio: false,
      selfBrowserSurface: "exclude",
      surfaceSwitching: "include",
      monitorTypeSurfaces: "exclude",
    });
    start(windowSource(stream));
  } catch (error) {
    if (error?.name !== "NotAllowedError") $("support").textContent = `Could not share: ${error.message || error}`;
  }
}

/** Open the floating window: a narrow, fixed-width strip whose height follows what it shows. */
async function popOut() {
  if (floating) return floating.window.focus();
  const size = lastSize[0] ? floatingSize(lastSize[0], lastSize[1]) : undefined;
  const opened = await openFloating(() => { floating = null; $("popout").textContent = "Float over the game"; }, size);
  if (!opened) return;
  floating = opened;
  $("popout").textContent = "Show the floating window";
  refreshViews();
}

function checkSupport() {
  const notes = [];
  if (!navigator.mediaDevices?.getDisplayMedia) notes.push("Use Chrome or Edge on a computer.");
  if (!("documentPictureInPicture" in window)) {
    $("popout").disabled = true;
    $("popout-note").textContent = "Needs Chrome or Edge.";
  }
  $("share").disabled = !navigator.mediaDevices?.getDisplayMedia;
  $("support").textContent = notes.join(" ") || "Nothing leaves your computer.";
}

/** `?preview=pip`: the floating window's view in a fixed-size box on the page, to look at it without a second window. */
function previewFloating() {
  const box = Object.assign(document.createElement("div"), { id: "pip-preview" });
  Object.assign(box.style, { position: "fixed", right: "16px", bottom: "16px", width: "300px", height: "420px", zIndex: 9, background: "#16203d", display: "flex", boxShadow: "0 8px 30px #000a", borderRadius: "8px", overflow: "hidden" });
  document.body.append(box);
  const view = new CardView(document, box, {
    emptyTitle: "Hover an item in the game",
    emptyText: "Its card shows up here.",
    icon: new URL("../assets/meso.png", location.href).href,
  });
  view.root.style.flex = "1";
  floating = { window: { focus() {}, close() {} }, view };
}

/** Once a source has run a while without reading a tooltip or window, say what usually is the matter. */
function watchForNothing() {
  setInterval(() => {
    const quiet = source && performance.now() - Math.max(startedAt, lastSeen) > NOTHING_AFTER_MS;
    const note = $("nothing");
    note.hidden = !quiet;
    if (quiet) {
      note.textContent = "Nothing read yet. Hover an item in the game window you shared.";
    }
  }, 1000);
}

let prices = null;

/** Load prices (the cached copy, then MesoWatch when 30 minutes have passed) and say where they came from. */
async function updatePrices() {
  if (!prices) return;
  const had = prices.items;
  await prices.refresh();
  const live = prices.items > 0;
  $("prices").textContent = live ? "MesoWatch prices" : "demo prices";
  if (live) $("version").textContent = `MapleLens ${lens.version} · ${prices.items.toLocaleString()} items`;
  if (live && !had) applyShopSettings();
}

async function main() {
  checkSupport();
  watchForNothing();
  pageView = new CardView(document, $("cards"), {
    emptyTitle: "Hover an item in the game",
    emptyText: "Its card appears here.",
    icon: new URL("../assets/meso.png", location.href).href,
  });
  $("share").addEventListener("click", share);
  $("stop").addEventListener("click", stop);
  $("popout").addEventListener("click", popOut);
  loadShopSettings();
  $("shop").addEventListener("change", e => { shopSettings.shop = e.target.checked; saveShopSettings(); applyShopSettings(); });
  $("notify-sales").addEventListener("change", e => { shopSettings.sales = e.target.checked; saveShopSettings(); applyShopSettings(); });
  $("notify-store").addEventListener("change", e => { shopSettings.store = e.target.checked; saveShopSettings(); applyShopSettings(); });
  const note = text => { $("discord-note").textContent = text; };
  $("discord-change").addEventListener("click", () => { note(""); showWebhook($("discord").hidden); });
  $("discord").addEventListener("keydown", e => { if (e.key === "Escape") showWebhook(); });
  $("discord").addEventListener("change", e => {
    const url = e.target.value.trim();
    if (!url) return;
    if (!WEBHOOK.test(url)) {
      note("That is not a Discord webhook URL");
      return;
    }
    shopSettings.webhook = url;
    saveShopSettings();
    showWebhook();
    applyShopSettings();
    note("Saved");
  });
  $("discord-clear").addEventListener("click", () => {
    shopSettings.webhook = "";
    saveShopSettings();
    showWebhook();
    applyShopSettings();
    note("Removed");
  });
  $("discord-test").addEventListener("click", async () => {
    note("Sending…");
    note((await lens.testNotify("discord", { webhook: shopSettings.webhook })).text);
  });
  const telegramNote = text => { $("telegram-note").textContent = text; };
  $("telegram-change").addEventListener("click", () => { telegramNote(""); showTelegram($("telegram").hidden); });
  $("telegram").addEventListener("keydown", e => { if (e.key === "Escape") showTelegram(); });
  $("telegram").addEventListener("change", e => {
    const text = e.target.value.trim();
    if (!text) return;
    const url = telegramWebhook(text);
    if (!url) {
      telegramNote("That is not a Telegram bot URL (api.telegram.org/bot…/sendMessage?chat_id=…) or token");
      return;
    }
    shopSettings.telegramWebhook = url;
    saveShopSettings();
    showTelegram();
    applyShopSettings();
    telegramNote(url.includes("chat_id=") ? "Saved" : "Saved. Send /start to your bot in Telegram, then press Test");
  });
  $("telegram-clear").addEventListener("click", () => {
    shopSettings.telegramWebhook = "";
    saveShopSettings();
    showTelegram();
    applyShopSettings();
    telegramNote("Removed");
  });
  $("telegram-test").addEventListener("click", async () => {
    const asked = shopSettings.telegramWebhook;
    telegramNote(asked.includes("chat_id=") ? "Sending…" : "Looking for your chat with the bot…");
    const result = await lens.testNotify("telegram", { telegramWebhook: asked });
    let text = result.text;
    // The completed URL is kept (unless another was pasted meanwhile) and offered on the clipboard to paste elsewhere.
    if (result.telegramWebhook && shopSettings.telegramWebhook === asked) {
      shopSettings.telegramWebhook = result.telegramWebhook;
      saveShopSettings();
      showTelegram();
      applyShopSettings();
      const copied = await navigator.clipboard?.writeText(result.telegramWebhook).then(() => true, () => false);
      if (copied) text += ". The full webhook URL is on your clipboard";
    }
    telegramNote(text);
  });
  try {
    lens = await Lens.create({
      onState,
      onCard: (which, image) => show(which, image),
      onError: message => { $("fps").textContent = message; },
      onNotify: result => { if (!result.ok) $(result.service === "Telegram" ? "telegram-note" : "discord-note").textContent = result.text; },
      ocr: true,
    });
    setPill("engine-pill", true, `engine ${lens.version}`);
    window.maplelensLens = lens; // for measuring and debugging from the console
    prices = new Prices(json => lens.loadCatalog(json));
    updatePrices();
    // A page left open asks again as soon as the 30 minutes are up (and no sooner).
    setInterval(() => { if (prices.due()) updatePrices(); }, 60 * 1000);
    applyShopSettings();
    $("version").textContent = `MapleLens ${lens.version} · ${lens.items} demo items`;
  } catch (error) {
    $("engine-pill").className = "pill warn";
    $("engine-pill").textContent = "engine failed to start";
    $("support").textContent = `The engine could not start: ${error.message || error}`;
    $("share").disabled = true;
  }
  if (params.get("preview") === "pip") previewFloating();
  if (params.has("demo")) start(await demoSource());
  if (params.has("video")) start(await videoSource(params.get("video")));
  if (params.has("capture")) start(await capturedVideoSource(params.get("capture")));
  if (params.has("frames")) start(await framesSource(params.get("frames")));
}

// `?report=label` (for measuring): post the numbers to the server that served the page, every few seconds.
if (params.has("report")) {
  setInterval(() => {
    window.gc?.(); // there only when Chrome runs with --js-flags=--expose-gc: the heap shown is then live data
    const body = JSON.stringify({ at: Date.now(), visible: document.visibilityState, label: params.get("report"), ...summary() });
    fetch("/report", { method: "POST", body }).catch(() => {});
  }, 5000);
}

main();
