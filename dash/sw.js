// Service worker minimo: habilita instalar como app (PWA) y una pantalla
// offline decente. NO cachea /state ni las APIs (siempre en vivo).
const SHELL = "comandos-shell-v12";
self.addEventListener("install", (e) => {
  self.skipWaiting();
  e.waitUntil(caches.open(SHELL).then((c) => c.addAll(["/", "/manifest.webmanifest"])));
});
self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== SHELL).map((k) => caches.delete(k))))
  );
  self.clients.claim();
});
self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  // APIs y acciones: SIEMPRE red, nunca cache (datos vivos, comandos reales).
  const live = [
    "/state", "/events", "/conf", "/prefs", "/ssh",
    "/tabs", "/tab-history", "/remote-state", "/remote-qr.png", "/webterm-token", "/term",
    "/operator", // Includes /operator/chat/stream and durable action results.
    "/session-", "/extension-usage", "/model/", "/usage/", "/providers"
  ];
  if (url.origin !== self.location.origin) return;
  if (e.request.method !== "GET" || live.some((p) => url.pathname.startsWith(p))) return;
  // Cache only the static shell/assets, never an unknown/new API response.
  if (url.pathname !== "/" && !/\.(?:html|css|js|png|svg|ico|woff2?|ttf|webmanifest)$/.test(url.pathname)) return;
  // La URL remota usa token en querystring: red primero sin guardar esa variante.
  if (url.pathname === "/" && url.searchParams.has("token")) {
    e.respondWith(fetch(e.request).catch(() => caches.match("/")));
    return;
  }
  // El shell: red primero, cache como respaldo offline.
  e.respondWith(
    fetch(e.request)
      .then((r) => { const cp = r.clone(); caches.open(SHELL).then((c) => c.put(e.request, cp)); return r; })
      .catch(() => caches.match(e.request).then((m) => m || caches.match("/")))
  );
});
