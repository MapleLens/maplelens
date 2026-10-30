// MapleLens Web: capture the game window (the page offers nothing else; the sample, video and frame-folder sources are
// test hooks started by a URL parameter), feed frames to the engine, show the cards in the page and, on request, in a picture-in-picture window that floats over the game.
// The training panel (EXP rates, potions) is drawn from the numbers the engine reads off the HUD.

import { Lens } from "./lens.js?v=eee7f010d696";
import { CardView, floatingSize, openFloating } from "./cards.js?v=eee7f010d696";
import { CardFeed } from "./cardfeed.js?v=eee7f010d696";
import { Prices } from "./prices.js?v=eee7f010d696";
import { drawMapleQr } from "./mapleqr.js?v=eee7f010d696";
import { StoreBook } from "./storebook.js?v=eee7f010d696";
import { KEY_SHAPE, newKey, phoneUrl, relay, seal } from "./phone/relay.js?v=eee7f010d696";

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
const TRAINING_KEY = "maplelens.web.training";
/** Past sessions listed under the training panel, newest first. */
const HISTORY_ROWS = 15;
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
/** The engine may take about a third of the time: after frames that took `ms` (the median of the last few), the next
 *  waits twice as long. The median, because the searches that run on their own clocks (a Character Info band every
 *  0.3 s, the shop each second) made one look in three slow, and with the mean a 1440p window was looked at 8 times a
 *  second instead of 10. */
const PACE = 2;
const PACE_FRAMES = 5;
/** With nothing to follow, a quarter-second between looks is fast enough (a tooltip takes longer than that to
 *  appear); while a tooltip or window is being read, the full rate. */
const IDLE_GAP_MS = 250;
const IDLE_AFTER_MS = 1500;

let lens = null;
let source = null; // {stop(), next(): Promise<ImageBitmap|null>, scale, label, fullFrame}
let insetIndex = 0;
let lockedUntil = 0;
let triedAt = 0;
let lastSize = [0, 0];
let floating = null; // {window, view}
let pageView = null;
/** What the page's card panel and the floating window show (the item card lingers, then joins the recent items). */
const feed = new CardFeed();
/** The worker's time on the last few frames (ms), for the pacing. */
const recentMs = [];
let lastState = null;
let lastSeen = 0; // when a tooltip or Character Info window was last in the state
let latest = { title: "", store: null, sharing: false };
/** The last item card the engine sent and what it is about, and the last rank panel's subject (the page draws that
 *  panel itself, with the character's photo). The engine sends one only when it changes, so an item hovered again shows
 *  the one kept. */
const images = { card: null, about: null, rank: null };
/** Which of them the last state showed. */
let live = { card: false, rank: false };
const stats = { shopEvents: [], frames: 0, dropped: 0, ms: [], grab: [], parts: [], gaps: [], longTasks: 0, started: 0, cards: 0, hovers: 0, titles: {}, mainMs: 0, pumpMs: 0 };
window.maplelensStats = () => summary();

function storage(action) {
  try { return action(window.localStorage); } catch { return undefined; } // private windows may refuse
}

/** What the player set for the shop, Discord and Telegram, kept in this browser. */
const shopSettings = { shop: true, sales: true, store: true, webhook: "", telegramWebhook: "", phone: "" };
/** The pairing QR code stays up this long at most: a streamer's screen is public, and the code is the key. */
const QR_SHOWN_MS = 120000;
/** How often the page asks the relay whether a phone has paired, while the QR code is up. */
const PAIR_POLL_MS = 4000;
/** The store book goes to the phones at most this often, and only when it changed. */
const UPLOAD_GAP_MS = 5000;
/** MapleLens checks in this often: the phone app shows the computer as offline when the check-ins stop. */
const CHECK_IN_MS = 60000;

function loadShopSettings() {
  try {
    Object.assign(shopSettings, JSON.parse(storage(s => s.getItem(SHOP_KEY)) || "{}"));
  } catch { /* damaged: the defaults stay */ }
  if (typeof shopSettings.webhook !== "string" || !WEBHOOK.test(shopSettings.webhook)) shopSettings.webhook = "";
  shopSettings.telegramWebhook = typeof shopSettings.telegramWebhook === "string" ? telegramWebhook(shopSettings.telegramWebhook) : "";
  if (typeof shopSettings.phone !== "string" || !KEY_SHAPE.test(shopSettings.phone)) shopSettings.phone = "";
  $("shop").checked = shopSettings.shop !== false;
  $("notify-sales").checked = shopSettings.sales !== false;
  $("notify-store").checked = shopSettings.store !== false;
  showWebhook();
  showTelegram();
  showPhone();
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

/** Phones paired with the key, as last asked; null before the first answer. */
let phones = null;

function showPhone() {
  const set = Boolean(shopSettings.phone);
  $("phone-state").textContent = !set ? "Not connected"
    : phones === null ? "Checking…"
    : phones ? `${phones} phone${phones > 1 ? "s" : ""} connected` : "No phone yet";
  // One phone per pairing: once it is connected, Remove is the way to connect another.
  $("phone-connect").hidden = Boolean(set && phones);
  $("phone-test").hidden = !set || !phones;
  $("phone-clear").hidden = !set;
}

/** The store as the phone app shows it. */
const book = new StoreBook(storage);
let lastUpload = 0;
let lastCheckIn = 0;
let lastFollowing = null;
let syncing = false;

/** Whether the page is watching the store right now (a shared window and the switch on). */
function following() {
  return Boolean(source && shopSettings.shop);
}

/** The relay's phone count; a drop means a phone disconnected itself, and the page says so. */
function setPhones(count) {
  if (phones > 0 && count < phones) {
    $("phone-note").textContent = count ? "A phone disconnected" : "Your phone disconnected. Press Connect to pair one again.";
  }
  // A phone that just joined gets the store straight away.
  if (count > (phones || 0)) book.changed = true;
  phones = count;
  showPhone();
}

/** Forgets the pairing key here: a new Connect makes a new key and QR code. */
function forgetPhoneKey(note) {
  hidePairing();
  shopSettings.phone = "";
  phones = null;
  saveShopSettings();
  showPhone();
  applyShopSettings();
  $("phone-note").textContent = note;
}

/** The relay's answer to an upload or a check-in: the phone count, or the key was removed (from another tab, or by a
 *  key holder) and is dead, so a QR code of it would only send phones to "Disconnected". */
function phoneAnswer(key, answer) {
  // An answer for a key removed meanwhile (Remove, then Connect again) says nothing about the new one.
  if (key !== shopSettings.phone) return;
  if (answer.status === 410) forgetPhoneKey("This pairing was removed. Press Connect to pair a phone again.");
  else if (answer.ok || answer.status === 404) setPhones(answer.ok ? answer.json.phones || 0 : 0);
}

/** MapleLens checking in: how many phones are paired (none once the pairing is gone). Resolves to the relay's answer. */
async function phoneStatus() {
  const key = shopSettings.phone;
  if (!key) return null;
  const answer = await relay("state/ping", { key });
  lastCheckIn = Date.now();
  phoneAnswer(key, answer);
  return answer;
}

/** Every few seconds: the sealed store book to the phones when it changed, else a check-in each minute. */
async function syncPhones() {
  const key = shopSettings.phone;
  if (syncing || !key) return;
  syncing = true;
  try {
    const now = Date.now();
    const watching = following();
    if (watching !== lastFollowing) book.changed = true;
    if (phones && book.changed && now - lastUpload >= UPLOAD_GAP_MS) {
      book.changed = false;
      const answer = await relay("state/put", { key, blob: await seal(key, book.snapshot(watching, now)) });
      lastUpload = lastCheckIn = now;
      if (answer.ok) lastFollowing = watching;
      else book.changed = true;
      phoneAnswer(key, answer);
    } else if (now - lastCheckIn >= CHECK_IN_MS - UPLOAD_GAP_MS) {
      // A sync early: Chrome runs a hidden tab's timers (the game covering it) once a minute after five minutes, and
      // a check-in due on the dot then missed every other wake-up, checking in every two minutes.
      await phoneStatus();
    }
  } finally {
    syncing = false;
  }
}

/** The pairing dialog while it is open: `view` is "code" (the QR code up), "connected" or "expired". */
let pairing = null;

const PAIR_BUTTONS = {
  code: ["Hide code", "I've scanned it"],
  connected: ["Done", "Send a test"],
  expired: ["Close", "Show it again"],
};

function pairView(view) {
  for (const [id, shown] of [["pair-code", "code"], ["pair-connected", "connected"], ["pair-expired", "expired"]]) {
    $(id).hidden = view !== shown;
  }
  [$("pair-back").textContent, $("pair-main").textContent] = PAIR_BUTTONS[view];
  $("pair-main").disabled = false;
  $("phone-dialog-title").textContent = view === "connected" ? "Phone connected" : "Connect your phone";
}

/** Stops waiting for a phone and wipes the code: it is the key. */
function stopWaiting() {
  if (pairing) {
    clearInterval(pairing.poll);
    clearInterval(pairing.tick);
    clearTimeout(pairing.timeout);
  }
  const qr = $("phone-qr");
  qr.getContext("2d").clearRect(0, 0, qr.width, qr.height);
}

/** Closes the dialog (Hide code, Done, Esc, a click outside, Remove). */
function hidePairing() {
  stopWaiting();
  pairing = null;
  if ($("phone-dialog").open) $("phone-dialog").close();
}

/** "Waiting for your phone · hides in 1:52", or what the last check found. */
function showWait() {
  if (pairing?.view !== "code") return;
  const left = Math.max(0, Math.ceil((pairing.since + QR_SHOWN_MS - Date.now()) / 1000));
  $("pair-wait").textContent = pairing.note || `Waiting for your phone · hides in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
}

function pairingEnded(view) {
  stopWaiting();
  pairing = { view };
  pairView(view);
  if (view === "connected") {
    $("pair-test-note").textContent = "Press Send a test to see one arrive";
    $("pair-test-note").className = "pair-status ok";
    $("phone-note").textContent = "Phone connected";
  }
}

/** A check while the code is up: a new phone ends the wait; a failed check says so (the next may get through). */
async function checkPairing(asked) {
  const waiting = pairing;
  const answer = await phoneStatus();
  if (pairing !== waiting || pairing?.view !== "code") return;
  if (phones > pairing.before) return pairingEnded("connected");
  pairing.note = !answer ? ""
    : answer.status === 0 ? "Can't reach MapleLens's relay: check your connection"
    : answer.status === 429 ? "The relay asks to slow down: checking again shortly"
    : !answer.ok && answer.status !== 404 ? `The relay answered HTTP ${answer.status}: still trying`
    : asked ? "No phone yet: on the phone, open the link and tap Turn on notifications"
    : pairing.note?.startsWith("No phone yet") ? pairing.note : "";
  showWait();
}

/** Shows the QR code (a new key the first time) and waits for a phone to pair with it. */
async function showPairing() {
  if (shopSettings.phone && phones) {
    $("phone-note").textContent = "A phone is already connected: Remove it first to connect another";
    return;
  }
  if (!shopSettings.phone) {
    shopSettings.phone = newKey();
    saveShopSettings();
    applyShopSettings();
  }
  const key = shopSettings.phone;
  const modules = await lens.phoneQr(phoneUrl(key));
  // The engine encodes https pairing addresses only: a phone could not open this page's address anyway.
  if (!modules.length) {
    $("phone-note").textContent = "Pairing needs the page over https (open it from the website)";
    return;
  }
  stopWaiting();
  await drawMapleQr($("phone-qr"), modules, new URL("../assets/meso.png", location.href).href);
  // Removed while the code was being drawn: its key is dead.
  if (key !== shopSettings.phone) return;
  const before = phones || 0;
  pairing = {
    view: "code",
    since: Date.now(),
    before,
    note: "",
    poll: setInterval(() => checkPairing(false), PAIR_POLL_MS),
    tick: setInterval(showWait, 1000),
    timeout: setTimeout(() => pairingEnded("expired"), QR_SHOWN_MS),
  };
  pairView("code");
  showWait();
  if (!$("phone-dialog").open) $("phone-dialog").showModal();
  showPhone();
}

/** A sold item's picture, once the engine found it: beside its sales in the log, the store book and the phone app. */
function showPicture(name, url) {
  book.setPicture(name, url);
  for (const row of $("store-log").querySelectorAll("li[data-item]")) {
    if (row.dataset.item === name && !row.querySelector(".icon")) row.querySelector(".t").after(pictureOf(url));
  }
}

function pictureOf(url) {
  return Object.assign(document.createElement("img"), { className: "icon px", src: url, alt: "", width: 24, height: 24 });
}

/** Pass the settings to the engine. */
function applyShopSettings() {
  if (!lens) return;
  lens.setShop(shopSettings.shop);
  const { webhook, telegramWebhook, phone, sales, store } = shopSettings;
  lens.setNotify({ webhook, telegramWebhook, phone, sales, store });
}

/** What the training card shows, kept in this browser. */
const trainingSettings = { exp: true };

function loadTrainingSettings() {
  try {
    Object.assign(trainingSettings, JSON.parse(storage(s => s.getItem(TRAINING_KEY)) || "{}"));
  } catch { /* damaged: the defaults stay */ }
  $("t-show-exp").checked = trainingSettings.exp !== false;
}

function applyTrainingSettings() {
  storage(s => s.setItem(TRAINING_KEY, JSON.stringify(trainingSettings)));
  lens?.trainingShow(trainingSettings.exp !== false);
}

/** 1920 -> "1.92k", 2_300_000 -> "2.3m" (as the cards write numbers). */
function compact(value) {
  if (value == null || !Number.isFinite(value)) return "—";
  for (const [divisor, unit] of [[1e9, "b"], [1e6, "m"], [1e3, "k"]]) {
    if (value >= divisor) {
      const scaled = value / divisor;
      return `${Number(scaled.toFixed(scaled < 10 ? 2 : scaled < 100 ? 1 : 0))}${unit}`;
    }
  }
  return String(Math.round(value));
}

/** 75 -> "1:15", 3725 -> "1:02:05". */
function clock(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  const two = n => String(n).padStart(2, "0");
  return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
}

/** "~14 min", "~2 h 5 min". */
function eta(seconds) {
  if (seconds == null) return "—";
  const minutes = Math.round(seconds / 60);
  if (minutes < 1) return "< 1 min";
  if (minutes < 60) return `~${minutes} min`;
  return minutes % 60 ? `~${Math.floor(minutes / 60)} h ${minutes % 60} min` : `~${minutes / 60} h`;
}

let trainingPaused = false;

function showTraining(t) {
  if (!t) return;
  trainingPaused = t.paused;
  const session = t.session || {};
  $("t-rate").textContent = compact(t.last_minute ?? t.recent_per_min);
  $("t-average").textContent = compact(t.average_per_min);
  $("t-best").textContent = session.best_minute ? compact(session.best_minute) : "—";
  $("t-level").textContent = eta(t.to_level_s);
  $("t-gained").textContent = session.gained ? `+${compact(session.gained)}` : "—";
  $("t-time").textContent = session.active_s ? clock(session.active_s) : "—";
  $("t-gauge").style.width = `${t.percent ?? 0}%`;
  $("t-pause").textContent = t.paused ? "Resume" : "Pause";
  $("training-tag").textContent = t.paused ? "paused" : !t.live ? "waiting for the EXP bar"
    : `${t.percent?.toFixed(2) ?? "?"}%${session.levels ? ` · ${session.levels} level${session.levels > 1 ? "s" : ""} up` : ""}`;
  const parts = [];
  if (t.map) parts.push(t.map_stat ? `${t.map}: +${compact(t.map_stat.gained)}` : t.map);
  if (t.percent_per_hour != null) parts.push(`${t.percent_per_hour.toFixed(1)}% of a level an hour`);
  if (session.lost) parts.push(`${compact(session.lost)} lost to deaths`);
  if (parts.length) $("t-note").textContent = parts.join(" · ");
  showPotions(t.potions);
}

/** The quickslots' potions this session: each one's icon with how many were used and picked up. */
let shownPotions = "";

function showPotions(potions) {
  const key = JSON.stringify(potions || {});
  if (key === shownPotions) return;
  shownPotions = key;
  const lines = potions?.lines || [];
  $("t-potions").hidden = !lines.length;
  $("t-potion-list").replaceChildren(...[...lines].sort((a, b) => b.used + b.picked - a.used - a.picked).map(line => {
    const chip = Object.assign(document.createElement("span"), { title: line.name });
    chip.append(Object.assign(document.createElement("img"), { src: line.icon, alt: line.name, loading: "lazy" }));
    if (line.used) chip.append(Object.assign(document.createElement("b"), { className: "used", textContent: `−${line.used}` }));
    if (line.picked) chip.append(Object.assign(document.createElement("b"), { className: "picked", textContent: `+${line.picked}` }));
    return chip;
  }));
}

function showHistory(history) {
  const list = $("t-history");
  const rows = (history || []).slice(-HISTORY_ROWS).reverse().map(session => {
    const row = document.createElement("li");
    const when = new Date(session.started * 1000);
    const time = Object.assign(document.createElement("span"), { className: "t", textContent: when.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) });
    const perMinute = session.active_s >= 30 ? ` · ${compact(session.gained * 60 / session.active_s)}/min` : "";
    const levels = session.levels ? ` · ${session.levels} level${session.levels > 1 ? "s" : ""}` : "";
    const where = session.maps?.length ? ` · ${[...session.maps].sort((a, b) => b.gained - a.gained)[0].map}` : "";
    row.append(time, document.createTextNode(`+${compact(session.gained)} in ${clock(session.active_s)}${perMinute}${levels}${where}`));
    return row;
  });
  if (rows.length) list.replaceChildren(...rows);
  else list.replaceChildren(Object.assign(document.createElement("li"), { className: "none", textContent: "None yet" }));
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
    row.append(time);
    if (event.kind === "sale" && event.item) {
      row.dataset.item = event.item;
      const known = book.pictures.get(event.item);
      if (known) row.append(pictureOf(known));
    }
    row.append(document.createTextNode(described[0]));
    log.prepend(row);
  }
  while (log.children.length > LOG_ROWS) log.lastChild.remove();
}

/** The Store Permit's status, in the words the Windows app's pill uses (they come from the engine, i.e. from core). */
function shopStatus(state) {
  const status = state.shop?.status;
  return shopSettings.shop && status?.text ? { text: status.text, tone: status.tone } : null;
}

/** A panel stays this long after what it is about was last seen: a window over the game, or a frame without it, must
 *  not make it blink. */
const PANEL_GRACE_MS = 20000;
const panelSeen = { training: -Infinity, store: -Infinity };

/** The training and store panels show only while they are about something: a training session, the player's own
 *  store. `state` is the engine's latest, or null when no game window is shared. */
function showPanels(state) {
  const now = performance.now();
  const about = {
    training: Boolean(state?.training?.active),
    // Unfollowed, the store is not watched and could never show up to be followed again.
    store: !shopSettings.shop || (state?.shop?.status?.status ?? "hidden") !== "hidden",
  };
  let any = false;
  for (const [name, on] of Object.entries(about)) {
    if (on) panelSeen[name] = now;
    const shown = on || (Boolean(state) && now - panelSeen[name] <= PANEL_GRACE_MS);
    if ($(`${name}-panel`).hidden === shown) $(`${name}-panel`).hidden = !shown;
    any ||= shown;
  }
  $("more-note").hidden = any;
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

/** The engine's parts, in the order of its state's `parts`. */
const PARTS = ["pixels", "tooltip", "info", "cards", "shop", "training"];

/** Average and 95th percentile of a list of timings (ms). */
function spread(list) {
  const sorted = [...list].sort((a, b) => a - b);
  const at = q => sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] : 0;
  return { avg: sorted.length ? sorted.reduce((a, b) => a + b, 0) / sorted.length : 0, p50: at(0.5), p95: at(0.95), max: sorted.at(-1) || 0 };
}

function summary() {
  const { avg, p50, p95, max } = spread(stats.ms);
  const seconds = stats.started ? (performance.now() - stats.started) / 1000 : 0;
  return {
    frames: stats.frames, dropped: stats.dropped, cards: stats.cards, hovers: stats.hovers, titles: stats.titles, seconds,
    fps: seconds ? stats.frames / seconds : 0,
    avgMs: avg, p50Ms: p50, p95Ms: p95, maxMs: max,
    // The time between looks (ms), the worker's turning a bitmap into pixels, and the engine's parts.
    gap: spread(stats.gaps), grab: spread(stats.grab), sourceFps: stats.sourceFps,
    parts: Object.fromEntries(PARTS.map((name, i) => [name, spread(stats.parts.map(p => p[i] || 0))])),
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

function refreshViews() {
  feed.update({ card: live.card ? images.card : null, rank: live.rank ? images.rank : null, about: images.about }, latest);
}

function show(which, image, about) {
  stats.cards += 1;
  if (which === "training") return feed.setTraining(image);
  if (which === "card") Object.assign(images, { card: image, about });
  else images.rank = about;
  refreshViews();
}

function drawBoxes(state, frameWidth, frameHeight) {
  const stage = $("boxes");
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

function onState(state, fps, ms, grab) {
  const t0 = performance.now();
  try {
    handleState(state, fps, ms, grab);
  } finally {
    stats.mainMs += performance.now() - t0;
  }
}

function handleState(state, fps, ms, grab) {
  lastState = state;
  stats.frames += 1;
  recentMs.push(ms + (grab || 0));
  if (recentMs.length > PACE_FRAMES) recentMs.shift();
  for (const [list, value] of [[stats.grab, grab || 0], [stats.parts, state.parts || []]]) {
    list.push(value);
    if (list.length > STATS_FRAMES) list.shift();
  }
  if (state.hover) {
    stats.hovers += 1;
    stats.titles[state.hover.title] = (stats.titles[state.hover.title] || 0) + 1;
  }
  stats.ms.push(ms);
  if (stats.ms.length > STATS_FRAMES) stats.ms.shift();
  setPill("tooltip-pill", Boolean(state.hover), state.hover ? `tooltip: ${state.hover.title}` : "no tooltip");
  setPill("window-pill", Boolean(state.info), state.info ? `Character Info${state.info.name ? `: ${state.info.name}` : ""}` : "no Character Info");
  showShopStatus(state);
  if (shopSettings.shop) book.update(state.shop);
  if (state.shop?.events?.length) {
    logShopEvents(state.shop.events, state.shop.owner);
    stats.shopEvents.push(...state.shop.events);
  }
  stats.shop = { looks: state.shop?.looks, undone: state.shop?.undone, ownerReads: state.shop?.owner_reads };
  const note = `${fps.toFixed(0)} looks a second · read ${ms.toFixed(0)} ms · UI ${state.scale.toFixed(2)}x${state.ocr ? " · reading new letters" : ""}`;
  $("fps").textContent = note;
  latest = {
    title: state.hover?.title || (state.info?.name ? `${state.info.name}'s ranks` : ""),
    // The store's status, else a training hint (the EXP bar covered): the Windows app's pill says the same.
    store: shopStatus(state) || state.hint || null,
    sharing: Boolean(source),
    trainingTitle: state.training?.map || "",
  };
  live = { card: Boolean(state.card), rank: Boolean(state.rank) };
  if (!state.training_card) feed.setTraining(null);
  showTraining(state.training);
  showPanels(state);
  refreshViews();
  drawBoxes(state, ...lastSize);
  // Auto mode: a detection proves the title bar guess right; keep it.
  if (state.hover || state.info) {
    lockedUntil = performance.now() + RELOCK_MS;
    lastSeen = performance.now();
  }
  // A tooltip still being read (its title, or an unknown one read alike several times) is looked at full rate too:
  // at the idle rate its card took over a second.
  if (state.tooltip_in_view) lastSeen = performance.now();
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

/** The time between looks (ms): the full rate while something is followed, a quarter second otherwise, and longer
 *  while the engine is slow. */
function lookGap() {
  const idle = performance.now() - lastSeen > IDLE_AFTER_MS;
  const ms = [...recentMs].sort((a, b) => a - b)[recentMs.length >> 1] || 0;
  return Math.max(idle ? IDLE_GAP_MS : 1000 / FPS, PACE * ms);
}

async function pump() {
  const view = $("view");
  const context = view.getContext("2d");
  let last = 0;
  // Looks are kept on a grid (`nextAt`, a gap after the one before), and a frame is taken when it is nearer its look's
  // time than the next frame would be. "A gap or more since the last look" missed most frames of a shared window,
  // which come at about the rate asked: each came a millisecond early, the next was taken, and a 10 fps share was
  // looked at 5 times a second.
  let nextAt = 0;
  let frameGap = 1000 / FPS; // between the source's frames, measured
  let lastFrame = 0;
  const due = () => performance.now() >= nextAt - frameGap / 2;
  // Asked by the source for every frame before it makes a bitmap of it: most of a 60 fps video's frames would be
  // thrown away, and a bitmap of every one of them cost more than reading the ones we keep.
  const offered = () => {
    const now = performance.now();
    if (lastFrame) frameGap += (Math.min(now - lastFrame, IDLE_GAP_MS) - frameGap) / 8;
    stats.sourceFps = 1000 / frameGap;
    lastFrame = now;
    return due();
  };
  while (source) {
    const bitmap = await source.next(offered);
    if (!bitmap || !source) break;
    if (!due()) {
      bitmap.close();
      continue;
    }
    const t0 = performance.now();
    lastSize = [bitmap.displayWidth ?? bitmap.width, bitmap.displayHeight ?? bitmap.height];
    if (!source.preview) {
      const k = Math.min(1, PREVIEW_PX / Math.max(...lastSize));
      const [w, h] = lastSize.map(n => Math.round(n * k));
      if (view.width !== w || view.height !== h) {
        view.width = w;
        view.height = h;
      }
      context.drawImage(bitmap, 0, 0, w, h);
    }
    // A look due while the engine is still on a slow one waits for it (a few ms) rather than for the next frame.
    if (lens.busy) await lens.idle();
    if (!source) {
      bitmap.close();
      break;
    }
    const accepted = lens.frame(bitmap, { inset: source.fullFrame ? 0 : inset(), scale: source.scale || manualScale() });
    if (accepted) {
      if (last) {
        stats.gaps.push(t0 - last);
        if (stats.gaps.length > STATS_FRAMES) stats.gaps.shift();
      }
      last = t0;
      // On time (early, or late by less than a frame): the grid holds. Later (the engine was busy, the source paused):
      // it starts again from now.
      nextAt = t0 - nextAt < frameGap ? nextAt + lookGap() : t0 + lookGap();
    } else stats.dropped += 1;
    stats.pumpMs += performance.now() - t0;
  }
}

/** The shared window shown as it plays (the page's own video, not the looks: those come 4-10 times a second). */
function showPreview(stream) {
  const live = $("live");
  live.srcObject = stream;
  live.hidden = !stream;
  $("view").hidden = Boolean(stream);
  if (stream) live.play().catch(() => {});
}

function start(newSource) {
  stop();
  source = newSource;
  lastSeen = performance.now();
  Object.assign(stats, { frames: 0, dropped: 0, ms: [], grab: [], parts: [], gaps: [], longTasks: 0, cards: 0, hovers: 0, titles: {}, mainMs: 0, pumpMs: 0, started: performance.now() });
  $("start").hidden = true;
  showPreview(source.preview || null);
  $("meter").hidden = false;
  $("source").textContent = source.label;
  pump();
}

function stop() {
  source?.stop();
  source = null;
  $("start").hidden = false;
  showPreview(null);
  $("view").hidden = true;
  $("meter").hidden = true;
  $("boxes").replaceChildren();
  $("source").textContent = "not connected";
  setPill("tooltip-pill", false, "no tooltip");
  setPill("window-pill", false, "no Character Info");
  $("fps").textContent = "";
  latest = { title: "", store: null, sharing: false };
  images.card = images.rank = images.about = null;
  live = { card: false, rank: false };
  feed.clear();
  feed.setTraining(null);
  refreshViews();
  showPanels(null);
}

/** Frames of a shared window, one at a time (VideoFrames where the browser has a track processor, else ImageBitmaps). */
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
        if (due()) return frame; // handed to the engine's worker as it is: it copies the pixels out itself
        frame.close();
      }
    };
  } else {
    const video = Object.assign(document.createElement("video"), { srcObject: stream, muted: true, playsInline: true });
    video.play();
    next = due => new Promise(function wait(resolve) {
      video.requestVideoFrameCallback(() => (due() ? createImageBitmap(video).then(resolve, () => resolve(null)) : wait(resolve)));
    });
  }
  return { label: track.label || "shared window", scale: 0, next, preview: stream, stop: () => stream.getTracks().forEach(t => t.stop()) };
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
  return { label, scale: 0, fullFrame: true, next, preview: video.captureStream(), stop: () => { video.pause(); video.removeAttribute("src"); if (typeof src !== "string") URL.revokeObjectURL(url); } };
}

/** A clip played into a media stream, which goes through exactly the code a shared window does (the track
 *  processor): for testing that path without sharing anything. `?capture=clip.mp4`. `&rate=10` delivers it at a
 *  window share's frame rate (Chrome sends a shared window at the rate asked, not the clip's), `&as=2560x1440` at
 *  another size: both drawn through a canvas. */
async function capturedVideoSource(url, rate = 0, size = "") {
  // A still picture (`?capture=frame.png&rate=10`) is a game window where nothing moves: a tooltip held.
  const still = /\.png$/i.test(url) && await createImageBitmap(await (await fetch(url)).blob());
  const video = Object.assign(document.createElement("video"), { src: still ? "" : url, muted: true, loop: true, playsInline: true });
  if (!still) await video.play();
  let stream;
  let timer = 0;
  if (rate || size || still) {
    const picture = still || video;
    const [width, height] = size ? size.split("x").map(Number) : [picture.width || video.videoWidth, picture.height || video.videoHeight];
    const canvas = Object.assign(document.createElement("canvas"), { width, height });
    const context = canvas.getContext("2d");
    if (still) timer = setInterval(() => context.drawImage(still, 0, 0, width, height), 1000 / (rate || FPS) / 2);
    else {
      video.requestVideoFrameCallback(function draw() {
        context.drawImage(video, 0, 0, width, height);
        video.requestVideoFrameCallback(draw);
      });
    }
    stream = canvas.captureStream(rate || FPS);
  } else stream = video.captureStream();
  const source = windowSource(stream);
  return { ...source, label: `${url.split("/").pop()} (as a stream)`, stop: () => { clearInterval(timer); source.stop(); video.pause(); } };
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

/** `?demo&blink`: the tooltip shows this long in every cycle. */
const BLINK_MS = 16000;
const BLINK_SHOWN_MS = 5000;

/** The sample screenshot with its tooltip and Character Info window painted over. */
function hiddenDemo(image) {
  const canvas = new OffscreenCanvas(image.width, image.height);
  const context = canvas.getContext("2d");
  context.drawImage(image, 0, 0);
  context.fillStyle = "#7d9b52";
  context.fillRect(470, 362, 470, 200);
  context.fillRect(32, 60, 512, 296);
  return canvas;
}

/** The sample screenshot, replayed: a Character Info window and an Emerald Ore tooltip, drawn at 1x. */
async function demoSource() {
  const image = await createImageBitmap(await (await fetch(new URL("demo-frame.png", import.meta.url))).blob());
  // `&blink`: the tooltip and the Character Info window come and go, as the pointer and the player would make them.
  const hidden = params.has("blink") ? hiddenDemo(image) : null;
  const started = performance.now();
  const rate = Number(params.get("rate")) || 0;
  if (rate) {
    // `&rate=10`: as a shared window delivers it, a stream at that frame rate through the track processor.
    const canvas = Object.assign(document.createElement("canvas"), { width: image.width, height: image.height });
    const context = canvas.getContext("2d");
    const stream = canvas.captureStream(rate);
    const timer = setInterval(() => {
      const away = hidden && (performance.now() - started) % BLINK_MS > BLINK_SHOWN_MS;
      context.drawImage(away ? hidden : image, 0, 0);
    }, 1000 / rate / 2);
    const source = windowSource(stream);
    return { ...source, label: "sample screenshot (as a stream)", scale: 1, fullFrame: true, stop: () => { clearInterval(timer); source.stop(); } };
  }
  let running = true;
  const next = async () => {
    await new Promise(r => setTimeout(r, 1000 / FPS));
    const away = hidden && (performance.now() - started) % BLINK_MS > BLINK_SHOWN_MS;
    return running ? createImageBitmap(away ? hidden : image) : null;
  };
  return { label: "sample screenshot", scale: 1, fullFrame: true, next, stop: () => { running = false; } };
}

async function share() {
  try {
    const stream = await navigator.mediaDevices.getDisplayMedia({
      // Twice the rate looked at: a window's frames come unevenly, and with one per look a late one costs a look.
      video: { displaySurface: "window", frameRate: { ideal: 2 * FPS, max: 30 } },
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

/** Open the floating window: one box in proportion to the screen (or the size the player last gave it). The page's
 *  own panel then says where the cards went, with a button that brings them back. */
async function popOut() {
  if (floating) return floating.window.focus();
  const opened = await openFloating(feed, () => {
    floating = null;
    pageView.setAway(null);
    $("popout").textContent = "Float over the game";
  }, floatingSize());
  if (!opened) return;
  floating = opened;
  pageView.setAway(() => floating?.window.close());
  $("popout").textContent = "Show the floating window";
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

/** `?preview=pip`: the floating window's view in a box on the page (`&size=340x560`, resizable by its corner), to look
 *  at it without a second window. */
function previewFloating() {
  const box = Object.assign(document.createElement("div"), { id: "pip-preview" });
  const size = floatingSize();
  const [width, height] = (params.get("size") || `${size.width}x${size.height}`).split("x").map(Number);
  Object.assign(box.style, { position: "fixed", right: "16px", bottom: "16px", width: `${width}px`, height: `${height}px`, zIndex: 9, background: "#16203d", display: "flex", boxShadow: "0 8px 30px #000a", borderRadius: "8px", overflow: "hidden", resize: "both" });
  document.body.append(box);
  const view = new CardView(document, box, feed, { compact: true });
  view.root.style.flex = "1";
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
  for (const button of document.querySelectorAll("[data-help]")) {
    const guide = $(button.dataset.help);
    button.addEventListener("click", () => guide.showModal());
    // A click on the dimmed backdrop (the dialog itself, outside its window) closes it too.
    guide.addEventListener("click", e => { if (e.target === guide) guide.close(); });
  }
  pageView = new CardView(document, $("cards"), feed, { page: true });
  $("share").addEventListener("click", share);
  $("stop").addEventListener("click", stop);
  $("popout").addEventListener("click", popOut);
  loadShopSettings();
  loadTrainingSettings();
  showPanels(null);
  $("t-show-exp").addEventListener("change", e => { trainingSettings.exp = e.target.checked; applyTrainingSettings(); });
  $("t-pause").addEventListener("click", () => {
    trainingPaused = !trainingPaused;
    $("t-pause").textContent = trainingPaused ? "Resume" : "Pause";
    lens?.trainingPause(trainingPaused);
  });
  $("t-new").addEventListener("click", () => lens?.trainingNewSession());
  $("shop").addEventListener("change", e => {
    shopSettings.shop = e.target.checked;
    saveShopSettings();
    applyShopSettings();
    showPanels(lastState && source ? lastState : null);
  });
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
  const phoneNote = text => { $("phone-note").textContent = text; };
  $("phone-connect").addEventListener("click", () => { phoneNote(""); if (lens) showPairing(); });
  const dialog = $("phone-dialog");
  // Esc, the close box and a click on the backdrop all close it, and the code goes with it. Watched on the `open`
  // attribute: a browser that holds back the close event (a hidden tab) must not leave the code polling.
  new MutationObserver(() => { if (!dialog.open) hidePairing(); }).observe(dialog, { attributes: true, attributeFilter: ["open"] });
  dialog.addEventListener("click", e => { if (e.target === dialog) dialog.close(); });
  $("pair-back").addEventListener("click", hidePairing);
  $("pair-main").addEventListener("click", async () => {
    const view = pairing?.view;
    if (view === "code") {
      pairing.note = "Checking…";
      showWait();
      await checkPairing(true);
    } else if (view === "connected") {
      const note = $("pair-test-note");
      $("pair-main").disabled = true;
      note.textContent = "Sending a test notification…";
      const result = await lens.testNotify("phone", { phone: shopSettings.phone });
      $("pair-main").disabled = false;
      note.textContent = result.ok ? "Sent: check your phone" : result.text;
      note.className = result.ok ? "pair-status ok" : "pair-status";
      // A refusal may mean the key was removed elsewhere (410): the check-in finds out and forgets it.
      if (!result.ok) phoneStatus();
    } else if (view === "expired") {
      showPairing();
    }
  });
  $("phone-test").addEventListener("click", async () => {
    phoneNote("Sending…");
    const result = await lens.testNotify("phone", { phone: shopSettings.phone });
    phoneNote(result.text);
    if (!result.ok) phoneStatus();
  });
  $("phone-clear").addEventListener("click", async () => {
    hidePairing();
    phoneNote("Removing…");
    // Forgets the pairing at the relay too: the phones are told and disconnect, and the old QR code is worthless.
    // The key is kept until the relay has done it: forgotten here only, it would still work for whoever holds it.
    const answer = await relay("unpair", { key: shopSettings.phone, remove: true });
    if (answer.ok || answer.status === 404 || answer.status === 410) {
      forgetPhoneKey("Removed: your phones were told and disconnected");
    } else {
      phoneNote(answer.status === 429 ? "Too many tries: wait a minute and press Remove again" : "Couldn't reach the relay: press Remove again");
    }
  });
  try {
    lens = await Lens.create({
      onState,
      onCard: (which, image, about) => show(which, image, about),
      onError: message => { $("fps").textContent = message; },
      onNotify: result => {
        const note = { Telegram: "telegram-note", Phone: "phone-note" }[result.service] || "discord-note";
        if (!result.ok) $(note).textContent = result.text;
      },
      onPicture: showPicture,
      onTrainingHistory: showHistory,
      ocr: true,
    });
    setPill("engine-pill", true, `engine ${lens.version}`);
    applyTrainingSettings();
    window.maplelensLens = lens; // for measuring and debugging from the console
    window.maplelensCards = feed;
    prices = new Prices(json => lens.loadCatalog(json, book.unpictured()));
    updatePrices();
    // A page left open asks again as soon as the 30 minutes are up (and no sooner).
    setInterval(() => { if (prices.due()) updatePrices(); }, 60 * 1000);
    applyShopSettings();
    phoneStatus();
    setInterval(syncPhones, UPLOAD_GAP_MS);
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
  if (params.has("capture")) start(await capturedVideoSource(params.get("capture"), Number(params.get("rate")) || 0, params.get("as") || ""));
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
