// The phone app: pairs with MapleLens (the key from the QR code's address, or scanned here), turns notifications on,
// then shows the store as MapleLens last saw it: while open, its status, mesos, numbers, who is inside, and its sales,
// chat and visitors; while closed, a stall waiting for the next one. Either way the stores before it are listed, each
// with its own sales, chat and visitors. The store's state comes from the relay sealed with the pairing key (relay.js
// `unseal`).
//
// Fastest path per phone: Android receives push in the browser tab itself (scan, tap, done; installing is optional).
// iPhone only lets home-screen apps receive push, so the page is installed once, and the app can pair by scanning.

import { pullToRefresh } from "./pull.js?v=8f28f7dbbfe7";
import { KEY_SHAPE, PAIR_CACHE, PAIR_REQUEST, relay, RELAY_URL, unseal } from "./relay.js?v=8f28f7dbbfe7";

const $ = id => document.getElementById(id);
const STORED_KEY = "maplelens.phone.key";
const TAB_KEY = "maplelens.phone.tab";
/** A pairing this phone switched away from but could not yet leave (no connection, too many tries): left later. */
const LEAVE_KEY = "maplelens.phone.leave";
/** The QR decoder for phones without BarcodeDetector (iPhone), loaded only when the player scans. Served from this
 *  site (scripts/vendor.mjs), never a CDN: the page holds the pairing key. */
const JSQR_URL = new URL("vendor/jsqr-1.4.0/jsQR.js", import.meta.url).href;
/** That exact file, as vendored. */
const JSQR_INTEGRITY = "sha384-b5Ya4Bq3qCyz39m2ISh+4DxjAIljdeFwK/BsXLuj9gugaNwAcj/ia15fxNZL9Nlx";
/** The iPhone install card (pwa-install), vendored like jsQR, and loaded only on an iPhone. */
const PWA_INSTALL_URL = new URL("vendor/pwa-install-0.6.3/pwa-install.bundle.js", import.meta.url).href;
const PWA_INSTALL_INTEGRITY = "sha384-Y/8ZP8Y+L1PZ9LFmLykD3Ao2udUqJ/jYlWWkJbjA+hQo5TpbXO1EcWQJuSt2RAGT";
const SCAN_EVERY_MS = 250;
/** The store is asked for this often while the app is on screen (a push also asks at once). */
const REFRESH_MS = 15000;
/** MapleLens checks in each minute; three missed check-ins and the computer is shown as offline. */
const OFFLINE_MS = 3 * 60 * 1000;
/** Item pictures are taken from this host only (what MapleLens names), so a picture cannot track the phone. */
const ICON_PREFIX = "https://maplestory.io/api/";
const ONBOARD_VIEWS = ["no-key", "removed", "scanner", "install", "unsupported", "enable", "switch"];

const ios = /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
const standalone = matchMedia("(display-mode: standalone)").matches || navigator.standalone === true;
const pushable = "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;
// Chrome offers to install only a manifest with a start_url ("start-url-not-valid" otherwise). Its installed app
// shares the browser's storage, so it finds the key kept there; an iPhone's does not, and keeps the one without.
if (!ios) document.querySelector('link[rel="manifest"]').href = "install.webmanifest";
const fromAddress = valid(new URLSearchParams(location.search).get("pair"));
// An installed app always opens the address it was installed from, so there the key saved by the last pairing wins;
// in a browser tab the address is a code just scanned and wins.
let key = standalone ? load(STORED_KEY) || fromAddress : fromAddress || load(STORED_KEY);
/** The player chose to keep the MapleLens this phone follows over the code in the address. */
let keptCurrent = false;
let installPrompt = null;
let scanning = null;
let refreshTimer = null;
let snapshot = null;
let seen = null;
/** Whether the last call to the relay failed for want of a connection (the phone's, not the computer's). */
let unreachable = false;
const LISTS = ["sales", "chat", "visits"];
let tab = LISTS.includes(load(TAB_KEY)) ? load(TAB_KEY) : "sales";
/** The past store open in its sheet, and the list shown there. */
let shownSession = null;
let sessionTab = "sales";

function valid(candidate) {
  return candidate && KEY_SHAPE.test(candidate) ? candidate : "";
}

function load(name) {
  try {
    return localStorage.getItem(name) || "";
  } catch {
    return "";
  }
}

function store(name, value) {
  try {
    if (value) localStorage.setItem(name, value);
    else localStorage.removeItem(name);
  } catch {}
}

/** An element with text (never HTML: names and chat are other players' words). */
function el(tag, props = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children.filter(c => c !== null && c !== undefined && c !== false));
  return node;
}

function show(view, state = "") {
  const board = view === "dashboard";
  $("onboard").hidden = board;
  $("dashboard").hidden = !board;
  $("settings-btn").hidden = !board;
  for (const id of ONBOARD_VIEWS) $(id).hidden = id !== view;
  $("state").textContent = state;
  if (!board) stopRefreshing();
}

let toastTimer = null;
function toast(text) {
  $("toast").textContent = text;
  $("toast").hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { $("toast").hidden = true; }, 3200);
}

async function registration() {
  return navigator.serviceWorker.register("sw.js", { scope: "./" });
}

/** The service worker re-pairs a renewed subscription by itself, with the key kept here. */
async function remember() {
  store(STORED_KEY, key);
  await (await caches.open(PAIR_CACHE)).put(PAIR_REQUEST, new Response(key));
  // Kept, the key is out of the address bar, where a shared link or a screenshot would give it away. Not in an iPhone
  // Safari tab: there the home-screen app is added from this address and needs the key in it.
  if (!ios || standalone) history.replaceState(null, "", "./");
}

async function vapidKey() {
  const response = await fetch(`${RELAY_URL}/key`);
  const { key: raw } = await response.json();
  return Uint8Array.from(atob(raw.replace(/-/g, "+").replace(/_/g, "/")), c => c.charCodeAt(0));
}

// ---------- pairing ----------

async function enable() {
  // Turning on, or switching a phone already connected to another MapleLens: the same steps, said where the tap was.
  const switching = !$("switch").hidden;
  const note = $(switching ? "switch-note" : "enable-note");
  const button = $(switching ? "switch-btn" : "enable-btn");
  button.disabled = true;
  note.textContent = "Asking for permission…";
  try {
    // Must come straight from the tap: iPhone refuses a request that does not.
    const permission = await Notification.requestPermission();
    if (permission !== "granted") {
      note.textContent =
        permission === "denied"
          ? "Notifications are blocked for this page. Allow them in the phone's settings, then try again."
          : "Notifications were not allowed.";
      return;
    }
    note.textContent = "Connecting…";
    const sw = await registration();
    await navigator.serviceWorker.ready;
    const subscription =
      (await sw.pushManager.getSubscription()) ||
      (await sw.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: await vapidKey() }));
    const before = load(STORED_KEY);
    const answer = await relay("pair", { key, subscription: subscription.toJSON() });
    if (answer.status === 410) {
      // Switching to a code removed since: the phone stays with the MapleLens it had (whose push subscription this is).
      if (!before || before === key) return removed();
      note.textContent = "That code was removed on its computer. Keep the current one, or scan a new code.";
      return;
    }
    if (!answer.ok) {
      note.textContent = pairProblem(answer.status);
      return;
    }
    // A phone follows one MapleLens: once the new pairing has it, it leaves the one it was in (later if it can't now).
    if (before && before !== key) store(LEAVE_KEY, before);
    await leaveOld(subscription);
    await remember();
    note.textContent = "";
    dashboard();
    toast("Connected. Notifications are on.");
  } catch (error) {
    note.textContent = `Could not turn notifications on (${error.message || error}).`;
  } finally {
    button.disabled = false;
  }
}

/** What a refused pairing means to the player. */
function pairProblem(status) {
  return status === 404 ? "This code belongs to another pairing. Scan the one MapleLens shows now."
    : status === 409 ? "This MapleLens already has 5 phones. Press Remove next to Phone on the computer, then connect again."
    : status === 429 ? "Too many tries. Wait a minute, then try again."
    : status === 0 ? "No connection. Check the phone's internet, then try again."
    : "Could not connect. Try again in a moment.";
}

/** Leaves the pairing this phone switched away from, once the relay answers (gone already counts). */
async function leaveOld(subscription) {
  const old = load(LEAVE_KEY);
  if (!old || !subscription) return;
  const answer = await relay("unpair", { key: old, endpoint: subscription.endpoint });
  if (answer.ok || answer.status === 404 || answer.status === 410) store(LEAVE_KEY, "");
}

/** Forget the key here (it no longer works anywhere) and say why. */
async function removed() {
  stopRefreshing();
  $("settings").close();
  const sw = await navigator.serviceWorker.getRegistration("./");
  await (await sw?.pushManager.getSubscription())?.unsubscribe();
  await caches.delete(PAIR_CACHE);
  store(STORED_KEY, "");
  store(LEAVE_KEY, "");
  key = "";
  snapshot = null;
  $("session").close();
  history.replaceState(null, "", "./");
  show("removed", "disconnected");
}

async function unpair() {
  if (!confirm("Disconnect this phone? It stops getting your store's notifications.")) return;
  const sw = await navigator.serviceWorker.getRegistration("./");
  const subscription = await sw?.pushManager.getSubscription();
  // Without a subscription the relay has nothing of this phone's to drop (and the other phones stay paired).
  if (subscription) await relay("unpair", { key, endpoint: subscription.endpoint });
  await subscription?.unsubscribe();
  await caches.delete(PAIR_CACHE);
  store(STORED_KEY, "");
  store(LEAVE_KEY, "");
  key = "";
  snapshot = null;
  $("settings").close();
  history.replaceState(null, "", "./");
  show("no-key", "not paired");
  $("scan-note").textContent = "Disconnected. MapleLens no longer sends to this phone.";
}

// ---------- scanning ----------

/** A decoder for camera frames: the browser's own where it has one (Android Chrome), else jsQR. */
async function decoder() {
  if ("BarcodeDetector" in window && (await BarcodeDetector.getSupportedFormats()).includes("qr_code")) {
    const detector = new BarcodeDetector({ formats: ["qr_code"] });
    return async video => (await detector.detect(video))[0]?.rawValue;
  }
  if (!window.jsQR) {
    await new Promise((resolve, reject) => {
      const script = el("script", { src: JSQR_URL, integrity: JSQR_INTEGRITY, onload: resolve, onerror: reject });
      document.head.append(script);
    });
  }
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d", { willReadFrequently: true });
  return async video => {
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    context.drawImage(video, 0, 0);
    return window.jsQR(context.getImageData(0, 0, canvas.width, canvas.height).data, canvas.width, canvas.height)?.data;
  };
}

function stopScanning() {
  if (!scanning) return;
  clearInterval(scanning.timer);
  scanning.stream.getTracks().forEach(track => track.stop());
  scanning = null;
}

/** The camera, until a MapleLens pairing code is in view. */
async function scan() {
  const back = $("removed").hidden ? "no-key" : "removed";
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" }, audio: false });
    const video = $("scan-video");
    video.srcObject = stream;
    await video.play();
    show("scanner", "scanning");
    const read = await decoder();
    scanning = {
      stream,
      timer: setInterval(async () => {
        if (!scanning || video.readyState < 2) return;
        const text = await read(video).catch(() => null);
        let found = "";
        try {
          found = valid(text && new URL(text).searchParams.get("pair"));
        } catch {}
        if (!found) return;
        stopScanning();
        key = found;
        history.replaceState(null, "", `?pair=${encodeURIComponent(key)}`);
        show("enable", "not connected");
        $("enable-note").textContent = "Code found.";
      }, SCAN_EVERY_MS),
    };
  } catch (error) {
    stopScanning();
    show(back, "not paired");
    $("scan-note").textContent = error?.name === "NotAllowedError" ? "The camera was not allowed. Scan the code with the camera app instead." : "The camera could not start. Scan the code with the camera app instead.";
  }
}

/** Pairs this phone's subscription again; "removed" clears the app, anything else waits for the next refresh. */
async function repair() {
  const sw = await navigator.serviceWorker.getRegistration("./");
  const subscription = await sw?.pushManager.getSubscription();
  if (!subscription) {
    $("settings").close();
    return show("enable", "not connected");
  }
  const answer = await relay("pair", { key, subscription: subscription.toJSON() });
  if (answer.status === 410) return removed();
  render();
  return answer;
}

/** A fresh push subscription in place of one the push service no longer delivers to, paired in its place. */
async function resubscribe() {
  const sw = await navigator.serviceWorker.getRegistration("./");
  const old = await sw?.pushManager.getSubscription();
  if (!sw || !old) return { ok: false, status: 404 };
  await old.unsubscribe();
  const fresh = await sw.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: await vapidKey() });
  return relay("pair", { key, subscription: fresh.toJSON(), replaces: old.endpoint });
}

// ---------- the dashboard ----------

function dashboard() {
  show("dashboard");
  $("waiting").hidden = Boolean(snapshot);
  $("store").hidden = !snapshot;
  refresh();
  startRefreshing();
}

function startRefreshing() {
  stopRefreshing();
  refreshTimer = setInterval(() => { if (!document.hidden) refresh(); }, REFRESH_MS);
}

function stopRefreshing() {
  clearInterval(refreshTimer);
  refreshTimer = null;
}

async function refresh() {
  if (!key) return;
  const answer = await relay("state/get", { key });
  unreachable = answer.status === 0;
  if (answer.status === 410) return removed();
  // Not paired any more without being removed (the push service dropped this phone's subscription): pair again
  // rather than forget the key; only a definite "removed" clears it.
  if (answer.status === 404) return repair();
  if (!answer.ok) return render();
  seen = answer.json.seen;
  const opened = answer.json.blob ? await unseal(key, answer.json.blob) : null;
  if (opened) snapshot = opened;
  render();
}

/** Between the parts of the line under the store's name; each part keeps its words together. */
const SEPARATOR = " · ";
const grouped = n => Math.round(n).toLocaleString("en-US");
const compact = n => (n >= 1e9 ? `${(n / 1e9).toFixed(1)}B` : n >= 1e6 ? `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M` : n >= 1e4 ? `${Math.round(n / 1e3)}K` : grouped(n));
const clock = t => new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
const DAY_MS = 86400000;

function ago(t, now = Date.now()) {
  const s = Math.max(0, (now - t) / 1000);
  if (s < 20) return "just now";
  if (s < 60) return `${Math.round(s)} s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(t).toLocaleDateString([], { month: "short", day: "numeric" });
}

function span(ms) {
  const minutes = Math.floor(ms / 60000);
  return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

function startOfDay(t) {
  const day = new Date(t);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
}

function dayLabel(t, now = Date.now()) {
  const today = startOfDay(now);
  if (t >= today) return "Today";
  if (t >= today - DAY_MS) return "Yesterday";
  return new Date(t).toLocaleDateString([], { weekday: "long", month: "short", day: "numeric" });
}

/** The pill, the line under the name and a hint when the player can do something about it. */
function storeState(s, now) {
  if (unreachable) {
    return { tone: "idle", pill: "No connection", hint: "This phone can't reach MapleLens right now. What you see is from earlier.", live: seen ? `Updated ${ago(seen, now)}` : "" };
  }
  if (!seen || now - seen > OFFLINE_MS) {
    return { tone: "idle", pill: "Computer offline", hint: "MapleLens on your computer stopped checking in. Your store may still be open; notifications come back with it.", live: seen ? `Last seen ${ago(seen, now)}` : "" };
  }
  // MapleLens checks in each minute and uploads whenever something changed, so the store is as of its last check-in.
  const updated = `Updated ${ago(seen, now)}`;
  // Closed, the stall below says what to do; only the time it closed is news.
  if (s.following && !s.open) {
    const last = s.sessions?.[0];
    return { tone: "idle", pill: "Closed", hint: null, live: (last ? `Closed ${ago(last.to, now)}${SEPARATOR}` : "") + updated };
  }
  if (!s.following) return { tone: "idle", pill: "Not watching", hint: "Share the game window in MapleLens Web to follow your store.", live: updated };
  if (!s.status) return { tone: "idle", pill: s.open ? "Open" : "No store", hint: s.open ? null : "MapleLens doesn't see your store on screen.", live: updated };
  const open = s.open && s.openedAt ? `Open for ${span(now - s.openedAt)}${SEPARATOR}` : "";
  if (s.status.tone === "ok") return { tone: "ok", pill: "Watching", hint: null, live: open + updated };
  if (s.status.tone === "warn") return { tone: "warn", pill: "Needs a look", hint: s.status.text, live: open + updated };
  return { tone: "idle", pill: s.open ? "Open" : "Closed", hint: s.open ? s.status.text : null, live: open + updated };
}

function render() {
  if ($("dashboard").hidden) return;
  $("waiting").hidden = Boolean(snapshot);
  $("store").hidden = !snapshot;
  if (!snapshot) return;
  const s = snapshot;
  const now = Date.now();
  $("owner").textContent = s.owner ? `${s.owner}'s store` : "Your store";
  const state = storeState(s, now);
  $("status").className = `state ${state.tone}`;
  $("status").textContent = state.pill;
  $("live").replaceChildren(...state.live.split(SEPARATOR).map((part, i) => el("span", { className: "part", textContent: i ? `· ${part}` : part })));
  $("hint").hidden = !state.hint;
  $("hint").className = state.tone === "warn" ? "hint-line warn" : "hint-line";
  // Sales are read off the Sold Items list: until it is open they arrive as "Something sold", so it is said loudly.
  if (s.status?.word === "need-sold-window") {
    $("hint").replaceChildren(el("b", { textContent: state.hint }), "Until it's open, sales show up as \"Something sold\" without the item.");
  } else {
    $("hint").textContent = state.hint || "";
  }
  $("balance").textContent = Number.isFinite(s.balance) ? grouped(s.balance) : "–";

  // The open store is the page; a closed one leaves the stall and the stores before it.
  const closed = !s.open;
  $("till").hidden = closed;
  $("closed-card").hidden = !closed;
  $("open-view").hidden = closed;
  $("week").hidden = !closed;
  renderPast(s.sessions || [], closed, now);
  if (closed) return;

  // Only this store's history: the ones before have their own page.
  const since = Number.isFinite(s.openedAt) ? s.openedAt : -Infinity;
  const sales = (s.sales || []).filter(x => x.t >= since);
  const chat = (s.chat || []).filter(x => x.t >= since);
  const visits = (s.visits || []).filter(x => x.t >= since);
  $("stat-sold").textContent = grouped(sales.length);
  $("stat-earned").textContent = compact(sales.reduce((sum, x) => sum + (x.mesos || 0), 0));
  $("stat-visitors").textContent = grouped(new Set(visits.filter(v => v.in).map(v => v.name)).size);

  const inside = s.following && seen && now - seen <= OFFLINE_MS ? s.inStore || [] : [];
  $("inside-count").textContent = inside.length ? `${inside.length} now` : "empty";
  $("inside").replaceChildren(
    ...(inside.length
      ? inside.map(name => el("span", { className: "chip" }, el("i", { textContent: (name.trim()[0] || "?").toUpperCase() }), name))
      : [el("span", { className: "empty-line", textContent: "Nobody right now" })]),
  );

  renderTabs("", { sales, chat, visits }, tab);
  const words = { sales: "No sales yet. They show up here the moment MapleLens sees them.", chat: "No messages yet.", visits: "No visitors yet." };
  $("list").replaceChildren(...listRows(tab, { sales, chat, visits }[tab], now, words[tab]));
}

/** A tab bar's counts and selection; `prefix` tells the dashboard's ("") from the sheet's ("session-"). */
function renderTabs(prefix, lists, current) {
  for (const name of LISTS) {
    $(`${prefix}n-${name}`).textContent = lists[name].length;
    $(`${prefix}tab-${name}`).setAttribute("aria-selected", String(current === name));
  }
}

function listRows(kind, entries, now, empty) {
  const rows = [];
  let day = null;
  for (const entry of entries) {
    const label = dayLabel(entry.t, now);
    if (label !== day) {
      day = label;
      rows.push(el("div", { className: "day", textContent: label }));
    }
    rows.push(kind === "sales" ? saleRow(entry) : kind === "chat" ? chatRow(entry) : visitRow(entry));
  }
  if (!rows.length) rows.push(el("div", { className: "blank" }, el("img", { className: "px", src: "../../assets/meso.png", alt: "" }), empty));
  return rows;
}

// ---------- past stores ----------

const CHEVRON = '<svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 5l7 7-7 7"/></svg>';
const plural = (n, one, many = `${one}s`) => `${grouped(n)} ${n === 1 ? one : many}`;
/** Hours without a leading zero: a past store's row has room for its two times and little else. */
const hours = t => new Date(t).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
const timeRange = session => `${hours(session.from)} – ${hours(session.to)}`;

/** The week's totals (closed) and every store kept, newest first; while one is open, only when there are some. */
function renderPast(sessions, closed, now) {
  const week = sessions.filter(session => session.to >= now - 7 * DAY_MS);
  $("week-stores").textContent = grouped(week.length);
  $("week-sold").textContent = grouped(week.reduce((sum, session) => sum + session.sold, 0));
  $("week-earned").textContent = compact(week.reduce((sum, session) => sum + session.earned, 0));
  $("past").hidden = !closed && !sessions.length;
  $("past-count").textContent = sessions.length ? plural(sessions.length, "store") : "none yet";
  $("sessions").replaceChildren(
    ...(sessions.length
      ? sessions.map(session => sessionRow(session, now))
      : [el("span", { className: "empty-line", textContent: "Each store shows up here once it closes, with what it sold and who came by." })]),
  );
}

function sessionRow(session, now) {
  const day = new Date(session.from);
  const label = dayLabel(session.from, now);
  const counts = [plural(session.sold, "sold", "sold"), plural(session.visitors, "visitor")];
  const button = el(
    "button",
    { className: "session", type: "button", ariaLabel: `${label}, ${timeRange(session)}` },
    el("div", { className: "cal" },
      el("span", { textContent: day.toLocaleDateString([], { weekday: "short" }) }),
      el("b", { textContent: String(day.getDate()) })),
    el("div", { className: "what" }, el("b", { textContent: timeRange(session) }), el("span", { textContent: counts.join(SEPARATOR) })),
    session.earned > 0 ? el("div", { className: "gain", textContent: `+${compact(session.earned)}` }) : el("div", { className: "gain none", textContent: "no sales" }),
  );
  button.insertAdjacentHTML("beforeend", CHEVRON);
  button.addEventListener("click", () => openSession(session));
  return button;
}

function openSession(session) {
  shownSession = session;
  sessionTab = "sales";
  renderSession();
  $("session").showModal();
  $("session").scrollTop = 0;
}

/** A past store's page: its numbers and the history kept from it. */
function renderSession() {
  const session = shownSession;
  if (!session || !snapshot) return;
  const now = Date.now();
  const within = entry => entry.t >= session.from && entry.t <= session.to;
  const lists = Object.fromEntries(LISTS.map(name => [name, (snapshot[name] || []).filter(within)]));
  $("session-title").textContent = dayLabel(session.from, now);
  $("session-when").textContent = [timeRange(session), `open ${span(session.to - session.from)}`, session.owner].filter(Boolean).join(SEPARATOR);
  $("session-sold").textContent = grouped(session.sold);
  $("session-earned").textContent = compact(session.earned);
  $("session-visitors").textContent = grouped(session.visitors);
  renderTabs("session-", lists, sessionTab);
  const words = { sales: "Nothing sold in this store.", chat: "No messages in this store.", visits: "Nobody came by." };
  const rows = listRows(sessionTab, lists[sessionTab], now, words[sessionTab]);
  // History is kept for a week and a busy day's worth: an older store's numbers outlive its rows.
  if (lists.sales.length < session.sold) rows.push(el("p", { className: "kept", textContent: "Older entries of this store are no longer kept." }));
  $("session-list").replaceChildren(...rows);
}

function saleRow(sale) {
  const picture = sale.icon && sale.icon.startsWith(ICON_PREFIX)
    ? el("div", { className: "slot" }, el("img", { src: sale.icon, alt: "", loading: "lazy", referrerPolicy: "no-referrer" }))
    : el("div", { className: "slot q", textContent: "?" });
  const name = sale.unseen ? "Something sold" : sale.qty > 1 ? `${sale.item} ×${grouped(sale.qty)}` : sale.item;
  const detail = sale.unseen ? `Item not read: Sold Items was closed · ${clock(sale.t)}` : `to ${sale.buyer} · ${clock(sale.t)}`;
  return el(
    "div",
    { className: "row" },
    picture,
    el("div", { className: "what" }, el("b", { textContent: name }), el("span", { textContent: detail })),
    Number.isFinite(sale.mesos) ? el("div", { className: "gain", textContent: `+${grouped(sale.mesos)}` }) : null,
  );
}

function chatRow(line) {
  return el(
    "div",
    { className: "bubble" },
    line.name ? el("b", { textContent: line.name }) : null,
    el("p", { textContent: line.text }),
    el("time", { textContent: clock(line.t) }),
  );
}

function visitRow(visit) {
  return el(
    "div",
    { className: "visit" },
    el("span", { className: `dot ${visit.in ? "in" : "out"}`, textContent: visit.in ? "→" : "←", ariaHidden: "true" }),
    el("span", { textContent: `${visit.name} ${visit.in ? "came in" : "left"}` }),
    el("time", { textContent: clock(visit.t) }),
  );
}

// ---------- installing on iPhone ----------

const b = text => el("b", { textContent: text });
const SHARE = "share";
const FINISH = ["Tap ", b("Add"), ", then open ", b("MapleLens"), " from your Home Screen."];

/** Which iPhone browser this is. Every one of them can add a page to the home screen since iOS 16.4, and what it adds
 *  is a Safari web app, so push works whichever added it; an app's own web view (Instagram, the Google app) cannot. */
function iosBrowser() {
  const ua = navigator.userAgent;
  if (/CriOS\//.test(ua)) return "chrome";
  if (/EdgiOS\//.test(ua)) return "edge";
  if (/FxiOS\//.test(ua)) return "firefox";
  if (/FBAN|FBAV|Instagram|Line\/|Snapchat|GSA\//.test(ua) || !/Safari\//.test(ua)) return "in-app";
  return "safari";
}

/** Each browser keeps Add to Home Screen somewhere else: Safari behind Share in its toolbar, Chrome behind Share in
 *  its address bar. Steps written for Safari sent Chrome users looking for a button that is not there. */
const INSTALL_STEPS = {
  safari: [
    ["Tap ", b("Share"), " ", SHARE, " in the toolbar. On iOS 26, tap ", b("•••"), " first."],
    ["Tap ", b("Add to Home Screen"), " (scroll down, or look under ", b("More"), ")."],
    FINISH,
  ],
  chrome: [
    ["Tap ", b("Share"), " ", SHARE, " in Chrome's address bar, or ", b("•••"), " then ", b("Share"), "."],
    ["Tap ", b("Add to Home Screen"), " (scroll down, or look under ", b("More"), ")."],
    FINISH,
  ],
  edge: [
    ["Tap ", b("•••"), " in Edge, then ", b("Share"), " ", SHARE, "."],
    ["Tap ", b("Add to Home Screen"), " (scroll down, or look under ", b("More"), ")."],
    FINISH,
  ],
  firefox: [
    ["Tap the menu ", b("☰"), " in Firefox, then ", b("Share"), " ", SHARE, "."],
    ["Tap ", b("Add to Home Screen"), "."],
    FINISH,
  ],
  "in-app": [
    ["Open this page in ", b("Safari"), ": tap ", b("•••"), " or ", SHARE, ", then ", b("Open in Safari"), ". An app's own browser cannot add it."],
    ["In Safari, tap ", b("Share"), " ", SHARE, ", then ", b("Add to Home Screen"), "."],
    FINISH,
  ],
};

function installSteps(list) {
  list.replaceChildren(
    ...INSTALL_STEPS[iosBrowser()].map(parts =>
      el("li", {}, ...parts.map(part =>
        part === SHARE ? $("share-icon").content.cloneNode(true) : typeof part === "string" ? part : part.cloneNode(true)))),
  );
}

/** iPhone lets no page add itself to the home screen and has no install prompt a page can call, so the page shows the
 *  card other sites show there (littlepolls.com): pwa-install, vendored, with this app's icon and its browser's steps.
 *  It opens by itself once a visit (it keeps that in sessionStorage); the page's own card lists the steps under it. */
function offerInstall() {
  installSteps($("install-steps"));
  if (document.querySelector("pwa-install")) return;
  const card = document.createElement("pwa-install");
  card.setAttribute("manifest-url", "manifest.webmanifest");
  card.setAttribute("icon", "apple-touch-icon.png");
  card.setAttribute("name", "MapleLens");
  card.setAttribute("description", "Store notifications");
  card.setAttribute("install-description", "On iPhone, notifications only reach apps on the Home Screen. Add MapleLens there to hear when an item sells or your store opens or closes.");
  card.setAttribute("disable-chrome", "");
  document.body.append(card);
  document.head.append(el("script", { src: PWA_INSTALL_URL, integrity: PWA_INSTALL_INTEGRITY }));
}

// ---------- settings ----------

function openSettings() {
  const permission = "Notification" in window ? Notification.permission : "denied";
  $("notify-state").textContent = permission === "granted" ? "On" : "Off";
  $("notify-state").className = permission === "granted" ? "pill-on" : "pill-off";
  $("notify-state-note").textContent = permission === "granted" ? "Sales and your store opening or closing" : "Allow notifications for MapleLens in the phone's settings";
  $("paired-key").textContent = `Store key ••••${key.slice(-4)}`;
  $("install-group").hidden = !installPrompt;
  $("test-note").textContent = "Check that this phone gets them";
  $("settings").showModal();
}

async function test() {
  $("test-btn").disabled = true;
  $("test-note").textContent = "Sending…";
  const answer = await relay("notify", {
    key,
    title: "MapleLens is connected",
    body: "Sold items and your store opening or closing will show up like this.",
    tag: "test",
  });
  $("test-btn").disabled = false;
  if (answer.status === 410) return removed();
  if (answer.status === 404) {
    // The relay dropped this phone: its push service said the subscription is gone, so a new one is made and paired.
    const again = await resubscribe().catch(() => ({ ok: false, status: -1 }));
    if (again.status === 410) return removed();
    $("test-note").textContent = again.ok ? "This phone was reconnected. Press Test again."
      : again.status === -1 ? "Notifications could not be turned back on. Open the phone's settings for MapleLens."
      : pairProblem(again.status);
    return;
  }
  $("test-note").textContent = answer.ok ? "Sent. It should arrive in a few seconds."
    : answer.status === 429 ? "Too many tests. Wait a minute, then try again."
    : answer.status === 0 ? "No connection. Check the phone's internet, then try again."
    : "Not sent. Try again in a moment.";
}

// ---------- start ----------

/** `?demo`: the dashboard with a made-up store, for the website and for looking at the design without a pairing;
 *  `?demo=closed` shows it closed. */
function demo() {
  const closed = new URLSearchParams(location.search).get("demo") === "closed";
  const now = Date.now();
  const min = 60000;
  const hour = 60 * min;
  const icon = id => `${ICON_PREFIX}GMS/40/item/${id}/icon`;
  const sales = [];
  const chat = [];
  const visits = [];
  const sessions = [];
  const sale = (t, item, id, qty, buyer, mesos) => sales.push({ t, item, qty, buyer, mesos, icon: icon(id) });
  const visit = (t, name, stay) => visits.push({ t, name, in: true }, { t: t + stay * min, name, in: false });
  const said = (t, name, text) => chat.push({ t, name, text });
  /** A past store from `from` to `to`, counted as the store book counts it. */
  const store = (from, to) => {
    const within = entry => entry.t >= from && entry.t <= to;
    const sold = sales.filter(within);
    sessions.push({
      from, to, owner: "ShopOwner", sold: sold.length, earned: sold.reduce((sum, x) => sum + (x.mesos || 0), 0),
      visitors: new Set(visits.filter(v => v.in && within(v)).map(v => v.name)).size,
    });
  };
  // Closed, the last store closed 25 minutes ago; open, the one before this store closed shortly before it opened.
  const opened = now - 134 * min;
  const base = closed ? now - 25 * min : opened - 6 * min;
  if (!closed) {
    sale(now - 3 * min, "Pet Food", 2120000, 20, "Tikko", 12400);
    sale(now - 41 * min, "Arrow for Bow", 2060000, 1000, "Frase", 250000);
    sales.push({ t: now - 77 * min, unseen: true, mesos: 35000 });
    said(now - 1 * min, "Tikko", "do you have more pet food?");
    said(now - 39 * min, "Frase", "ty for the arrows!");
    visits.push({ t: now - 4 * min, name: "Wina", in: true }, { t: now - 5 * min, name: "Tikko", in: true });
    visit(now - 42 * min, "Frase", 2);
  }
  sale(base - 8 * min, "White Potion", 2000002, 200, "Lumina", 220000);
  sale(base - 35 * min, "Subi Throwing-Stars", 2070000, 1, "Kaito", 180000);
  sales.push({ t: base - 62 * min, unseen: true, mesos: 45000 });
  sale(base - 95 * min, "Steel Ore", 4010001, 30, "Mobbie", 96000);
  sale(base - 118 * min, "Blue Potion", 2000003, 100, "Frase", 32000);
  visit(base - 10 * min, "Lumina", 4);
  visit(base - 37 * min, "Kaito", 4);
  visit(base - 50 * min, "Tikko", 2);
  visit(base - 97 * min, "Mobbie", 7);
  visit(base - 120 * min, "Frase", 5);
  said(base - 9 * min, "Lumina", "can i get 200 whites?");
  said(base - 34 * min, "Kaito", "nice price on subis");
  said(base - 96 * min, "Mobbie", "ty!");
  store(base - 130 * min, base);
  const yesterday = base - 26 * hour;
  sale(yesterday - 20 * min, "Arrow for Crossbow", 2061000, 2000, "Bowie", 160000);
  sale(yesterday - 70 * min, "Red Potion", 2000000, 300, "Wina", 15000);
  sale(yesterday - 150 * min, "Mithril Ore", 4010002, 10, "Tikko", 85000);
  visit(yesterday - 22 * min, "Bowie", 3);
  visit(yesterday - 72 * min, "Wina", 3);
  visit(yesterday - 100 * min, "Lumina", 1);
  visit(yesterday - 152 * min, "Tikko", 3);
  said(yesterday - 21 * min, "Bowie", "2k crossbow arrows pls");
  store(yesterday - 3 * hour, yesterday);
  const earlier = base - 74 * hour;
  sale(earlier - 15 * min, "Pet Food", 2120000, 10, "Frase", 6200);
  visit(earlier - 17 * min, "Frase", 3);
  visit(earlier - 30 * min, "Kaito", 1);
  store(earlier - 45 * min, earlier);
  store(base - 122 * hour, base - 121 * hour);
  const newest = (a, b) => b.t - a.t;
  snapshot = {
    v: 1, at: now - 8000, following: true, owner: "ShopOwner", open: !closed, openedAt: closed ? null : opened, balance: 1839311,
    status: closed ? { word: "not-open", text: "Store not open", tone: "idle" } : { word: "watching", text: "Store detected", tone: "ok" },
    inStore: closed ? [] : ["Tikko", "Wina"],
    sales: sales.sort(newest), chat: chat.sort(newest), visits: visits.sort(newest), sessions,
  };
  seen = now - 8000;
  key = "demo0000000000000.demo";
  show("dashboard");
  render();
  $("settings-btn").hidden = true;
}

async function start() {
  if (new URLSearchParams(location.search).has("demo")) return demo();
  // iPhone allows push only to home-screen apps, whatever the Safari tab exposes.
  if (ios && !standalone) {
    show("install", "add to home screen");
    return offerInstall();
  }
  if (!pushable) return show("unsupported", "not available");
  // Registered before pairing too: Chrome's install prompt waits for a service worker that handles fetches. Not
  // awaited yet: a phone that cannot register still sees how to pair, and learns why only when it tries.
  const worker = registration();
  worker.catch(() => {});
  $("scan-btn").hidden = $("rescan-btn").hidden = !navigator.mediaDevices?.getUserMedia;
  if (!key) return show("no-key", "not paired");
  // A code for another MapleLens than the one this phone follows: only a tap switches it (a link anyone can post
  // must not take the phone over).
  const stored = load(STORED_KEY);
  if (!standalone && !keptCurrent && fromAddress && stored && fromAddress !== stored) return show("switch", "already connected");
  const sw = await worker;
  const subscription = await sw.pushManager.getSubscription();
  const paired = Notification.permission === "granted" && subscription && (await caches.has(PAIR_CACHE));
  if (!paired) return show("enable", "not connected");
  await remember();
  dashboard();
  // Re-pairing is harmless (the relay keeps one entry per phone, and writes nothing for an unchanged one) and heals
  // a pairing the relay dropped; a key removed on the computer answers 410.
  const answer = await relay("pair", { key, subscription: subscription.toJSON() });
  if (answer.status === 410) return removed();
  if (answer.status === 409) {
    // Not in the pairing, and no room in it: say so rather than show a dashboard that gets no notifications.
    show("enable", "not connected");
    $("enable-note").textContent = pairProblem(409);
    return;
  }
  await leaveOld(subscription);
}

// Android Chrome offers its install dialog when it judges the page installable: a home-screen icon, optional there.
// It may come after the first view is shown, so the button above every view appears when it does. Not prevented:
// Chrome's own offer (the bar at the bottom) still shows, and the event's prompt() still works for our button.
addEventListener("beforeinstallprompt", event => {
  installPrompt = event;
  $("install-landing").hidden = false;
});
addEventListener("appinstalled", () => {
  installPrompt = null;
  $("install-landing").hidden = $("install-group").hidden = true;
});
navigator.serviceWorker?.addEventListener("message", ({ data }) => {
  if (data?.type === "unpaired") removed();
  else if (data?.type === "pushed" && !$("dashboard").hidden) refresh();
});
document.addEventListener("visibilitychange", () => { if (!document.hidden && !$("dashboard").hidden) refresh(); });

// A prompt can be shown once: both buttons go until Chrome offers another.
async function install() {
  installPrompt?.prompt();
  await installPrompt?.userChoice;
  installPrompt = null;
  $("install-landing").hidden = $("install-group").hidden = true;
}
$("install-btn").addEventListener("click", install);
$("install-landing-btn").addEventListener("click", install);
$("enable-btn").addEventListener("click", enable);
$("switch-btn").addEventListener("click", enable);
$("keep-btn").addEventListener("click", () => {
  keptCurrent = true;
  key = load(STORED_KEY);
  history.replaceState(null, "", "./");
  start();
});
$("test-btn").addEventListener("click", test);
$("unpair-btn").addEventListener("click", unpair);
$("scan-btn").addEventListener("click", scan);
$("rescan-btn").addEventListener("click", scan);
$("scan-cancel").addEventListener("click", () => {
  stopScanning();
  show(key ? "enable" : "no-key", "not paired");
});
$("settings-btn").addEventListener("click", openSettings);
$("settings-close").addEventListener("click", () => $("settings").close());
// A tap on the dimmed backdrop closes the sheet, as on every phone.
$("settings").addEventListener("click", event => { if (event.target === $("settings")) $("settings").close(); });
for (const name of LISTS) {
  $(`tab-${name}`).addEventListener("click", () => {
    tab = name;
    store(TAB_KEY, name);
    render();
  });
  $(`session-tab-${name}`).addEventListener("click", () => {
    sessionTab = name;
    renderSession();
  });
}
$("session-close").addEventListener("click", () => $("session").close());
$("session").addEventListener("click", event => { if (event.target === $("session")) $("session").close(); });
// The store asked again on the dashboard; elsewhere the pairing's steps are looked at again (a code scanned with the
// camera app, notifications allowed in the phone's settings).
pullToRefresh(document.querySelector("main"), $("ptr"), async () => {
  if (new URLSearchParams(location.search).has("demo")) return demo();
  if (!$("dashboard").hidden) {
    await refresh();
    if (unreachable) toast("No connection. Showing what MapleLens sent earlier.");
    return;
  }
  await start();
}, () => !scanning && !$("settings").open && !$("session").open);
start().catch(error => {
  show("unsupported", "error");
  $("unsupported").querySelector("p").textContent = `Something went wrong: ${error.message || error}`;
});
