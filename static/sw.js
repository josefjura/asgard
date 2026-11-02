// Caches the static assets and the offline page, and shows pushes. Data pages always need the network.
// build.rs fills in the placeholders below, so any asset change installs a new worker.
const CACHE = "asgard-{{VERSION}}";
const PRECACHE = {{PRECACHE}};
const OFFLINE_URL = "/offline";

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll([...PRECACHE, OFFLINE_URL]))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin === self.location.origin && PRECACHE.includes(url.pathname)) {
    event.respondWith(caches.match(request).then((cached) => cached ?? fetch(request)));
  } else if (request.mode === "navigate") {
    event.respondWith(fetch(request).catch(offlineResponse));
  }
});

async function offlineResponse() {
  const cached = await caches.match(OFFLINE_URL);
  return (
    cached ??
    new Response("You're offline. Check your connection, then try again.", {
      status: 503,
      headers: { "Content-Type": "text/plain; charset=utf-8" },
    })
  );
}

// A push carries the notification as JSON: { title, body, url }, url being the tab to open.
self.addEventListener("push", (event) => {
  const { title, body, url } = event.data?.json() ?? {};
  event.waitUntil(
    self.registration.showNotification(title ?? "Asgard", {
      body,
      icon: "{{ICON_192}}",
      badge: "{{BADGE_96}}",
      data: { url: url ?? "/" },
    }),
  );
});

// Tapping a notification brings an open Asgard window to its tab, or opens one there.
self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = new URL(event.notification.data?.url ?? "/", self.location.origin).href;
  event.waitUntil(openOn(url));
});

async function openOn(url) {
  const [open] = await self.clients.matchAll({ type: "window" });
  if (!open) return self.clients.openWindow(url);
  const focused = await open.focus();
  if (focused.url === url) return focused;
  try {
    return await focused.navigate(url);
  } catch {
    return self.clients.openWindow(url);
  }
}
