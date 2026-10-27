// The phone page's service worker: shows what the relay pushes (MapleLens's sales and store changes) and, when the
// browser renews the push subscription, pairs the new one with the key the page left in the cache.

// A classic worker (module service workers are newer than push on some phones), so relay.js's values are repeated.
const RELAY_URL = "https://maplelens-push.prices-proxy.workers.dev";
const PAIR_CACHE = "maplelens-phone";
const PAIR_REQUEST = "pair-key";

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", event => event.waitUntil(self.clients.claim()));

self.addEventListener("push", event => {
  let message = {};
  try {
    message = event.data ? event.data.json() : {};
  } catch {
    message = { title: "MapleLens", body: event.data?.text() || "" };
  }
  const options = {
    body: message.body || "",
    icon: message.icon || "icon-192.png",
    badge: "badge.png",
    data: { url: self.registration.scope },
  };
  // A tag replaces the last message with the same tag ("store opened" then "store closed"), and still alerts.
  if (message.tag) Object.assign(options, { tag: message.tag, renotify: true });
  event.waitUntil(
    (async () => {
      // Removed in MapleLens on the computer: the key is dead, so forget it; an open app shows why at once. Only the
      // pairing this phone holds now: one it switched away from may still say goodbye.
      let unpaired = false;
      if (message.tag === "unpaired") {
        const stored = await (await caches.open(PAIR_CACHE)).match(PAIR_REQUEST);
        const held = stored ? await stored.text() : "";
        const other = Boolean(held) && Boolean(message.pairing) && !held.startsWith(`${message.pairing}.`);
        unpaired = !other;
        if (unpaired) await caches.delete(PAIR_CACHE);
        else Object.assign(options, { body: "An earlier MapleLens connection was removed. This phone is still connected.", tag: "old-pairing" });
      }
      const open = await self.clients.matchAll({ type: "window" });
      for (const client of open) client.postMessage({ type: unpaired ? "unpaired" : "pushed" });
      await self.registration.showNotification(message.title || "MapleLens", options);
    })(),
  );
});

self.addEventListener("notificationclick", event => {
  event.notification.close();
  event.waitUntil(
    (async () => {
      const open = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      const page = open.find(client => client.url.startsWith(self.registration.scope));
      if (page) return page.focus();
      return self.clients.openWindow(event.notification.data?.url || self.registration.scope);
    })(),
  );
});

// Browsers renew subscriptions now and then; without pairing the new one the phone would go quiet.
self.addEventListener("pushsubscriptionchange", event => {
  event.waitUntil(
    (async () => {
      const stored = await (await caches.open(PAIR_CACHE)).match(PAIR_REQUEST);
      if (!stored) return;
      const key = await stored.text();
      // Firefox gives no old subscription: the relay's key is asked again.
      const applicationServerKey = event.oldSubscription?.options?.applicationServerKey || (await vapidKey());
      const subscription =
        event.newSubscription ||
        (await self.registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey }));
      // `replaces`: the same phone renewing, so the others get no "new phone" alert and the old one frees its place.
      const answer = await fetch(`${RELAY_URL}/pair`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ key, subscription, replaces: event.oldSubscription?.endpoint }),
      });
      // Without the old address (Firefox) a full pairing has no room for the renewed one: the phone would go quiet.
      if (answer.status === 409) {
        await self.registration.showNotification("MapleLens stopped notifying this phone", {
          body: "Its connection was renewed, and 5 phones are already connected. Press Remove next to Phone on the computer, then connect again.",
          icon: "icon-192.png",
          badge: "badge.png",
          data: { url: self.registration.scope },
        });
      }
    })(),
  );
});

async function vapidKey() {
  const { key: raw } = await (await fetch(`${RELAY_URL}/key`)).json();
  return Uint8Array.from(atob(raw.replace(/-/g, "+").replace(/_/g, "/")), c => c.charCodeAt(0));
}
