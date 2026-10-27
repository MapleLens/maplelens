// The phone app: pairs with MapleLens (the key from the QR code's address, or scanned here), turns notifications on,
// then shows the store as MapleLens last saw it: status, mesos, today's numbers, who is inside, and the sales, chat
// and visitor history. The store's state comes from the relay sealed with the pairing key (relay.js `unseal`).
//
// Fastest path per phone: Android receives push in the browser tab itself (scan, tap, done; installing is optional).
// iPhone only lets home-screen apps receive push, so the page is installed once, and the app can pair by scanning.

import { KEY_SHAPE, PAIR_CACHE, PAIR_REQUEST, relay, RELAY_URL, unseal } from "./relay.js?v=d5e2993c70a9";

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
let tab = ["sales", "chat", "visits"].includes(load(TAB_KEY)) ? load(TAB_KEY) : "sales";

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
  if (t >= today - 86400000) return "Yesterday";
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

  const today = startOfDay(now);
  const sales = s.sales || [];
  const chat = s.chat || [];
  const visits = s.visits || [];
  const soldToday = sales.filter(x => x.t >= today);
  $("stat-sold").textContent = grouped(soldToday.length);
  $("stat-earned").textContent = compact(soldToday.reduce((sum, x) => sum + (x.mesos || 0), 0));
  $("stat-visitors").textContent = grouped(new Set(visits.filter(v => v.t >= today && v.in).map(v => v.name)).size);

  const inside = s.following && seen && now - seen <= OFFLINE_MS ? s.inStore || [] : [];
  $("inside-count").textContent = inside.length ? `${inside.length} now` : "empty";
  $("inside").replaceChildren(
    ...(inside.length
      ? inside.map(name => el("span", { className: "chip" }, el("i", { textContent: (name.trim()[0] || "?").toUpperCase() }), name))
      : [el("span", { className: "empty-line", textContent: "Nobody right now" })]),
  );

  $("n-sales").textContent = sales.length;
  $("n-chat").textContent = chat.length;
  $("n-visits").textContent = visits.length;
  for (const name of ["sales", "chat", "visits"]) $(`tab-${name}`).setAttribute("aria-selected", String(tab === name));
  renderList({ sales, chat, visits }[tab], now);
}

function renderList(entries, now) {
  const rows = [];
  let day = null;
  for (const entry of entries) {
    const label = dayLabel(entry.t, now);
    if (label !== day) {
      day = label;
      rows.push(el("div", { className: "day", textContent: label }));
    }
    rows.push(tab === "sales" ? saleRow(entry) : tab === "chat" ? chatRow(entry) : visitRow(entry));
  }
  if (!rows.length) {
    const words = { sales: "No sales yet. They show up here the moment MapleLens sees them.", chat: "No messages yet.", visits: "No visitors yet." };
    rows.push(el("div", { className: "blank" }, el("img", { className: "px", src: "../../assets/meso.png", alt: "" }), words[tab]));
  }
  $("list").replaceChildren(...rows);
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

/** `?demo`: the dashboard with a made-up store, for the website and for looking at the design without a pairing. */
function demo() {
  const now = Date.now();
  const min = 60000;
  const icon = id => `${ICON_PREFIX}GMS/40/item/${id}/icon`;
  snapshot = {
    v: 1, at: now - 8000, following: true, owner: "ShopOwner", open: true, openedAt: now - 134 * min, balance: 1839311,
    status: { word: "watching", text: "Store detected", tone: "ok" },
    inStore: ["Tikko", "Wina"],
    sales: [
      { t: now - 3 * min, item: "Pet Food", qty: 20, buyer: "Tikko", mesos: 12400, icon: icon(2120000) },
      { t: now - 41 * min, item: "Arrow for Bow", qty: 1000, buyer: "Frase", mesos: 250000, icon: icon(2060000) },
      { t: now - 77 * min, unseen: true, mesos: 35000 },
      { t: now - 26 * 60 * min, item: "Red Potion", qty: 100, buyer: "Mobbie", mesos: 5000, icon: icon(2000000) },
    ],
    chat: [
      { t: now - 1 * min, name: "Tikko", text: "do you have more pet food?" },
      { t: now - 39 * min, name: "Frase", text: "ty for the arrows!" },
    ],
    visits: [
      { t: now - 4 * min, name: "Wina", in: true },
      { t: now - 5 * min, name: "Tikko", in: true },
      { t: now - 40 * min, name: "Frase", in: false },
      { t: now - 42 * min, name: "Frase", in: true },
    ],
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
  if (ios && !standalone) return show("install", "add to home screen");
  if (!pushable) return show("unsupported", "not available");
  $("scan-btn").hidden = $("rescan-btn").hidden = !navigator.mediaDevices?.getUserMedia;
  if (!key) return show("no-key", "not paired");
  // A code for another MapleLens than the one this phone follows: only a tap switches it (a link anyone can post
  // must not take the phone over).
  const stored = load(STORED_KEY);
  if (!standalone && !keptCurrent && fromAddress && stored && fromAddress !== stored) return show("switch", "already connected");
  const sw = await registration();
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
addEventListener("beforeinstallprompt", event => {
  event.preventDefault();
  installPrompt = event;
});
navigator.serviceWorker?.addEventListener("message", ({ data }) => {
  if (data?.type === "unpaired") removed();
  else if (data?.type === "pushed" && !$("dashboard").hidden) refresh();
});
document.addEventListener("visibilitychange", () => { if (!document.hidden && !$("dashboard").hidden) refresh(); });

$("install-btn").addEventListener("click", async () => {
  installPrompt?.prompt();
  await installPrompt?.userChoice;
  installPrompt = null;
  $("install-group").hidden = true;
});
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
for (const name of ["sales", "chat", "visits"]) {
  $(`tab-${name}`).addEventListener("click", () => {
    tab = name;
    store(TAB_KEY, name);
    render();
  });
}
start().catch(error => {
  show("unsupported", "error");
  $("unsupported").querySelector("p").textContent = `Something went wrong: ${error.message || error}`;
});
