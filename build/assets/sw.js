/* PractiCal service worker: Web Push reminders (plan D12). */
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()));
self.addEventListener('push', e => {
  let data = {};
  try { data = e.data ? e.data.json() : {}; } catch (_) { data = { title: e.data && e.data.text() }; }
  const title = data.title || 'Erinnerung';
  e.waitUntil(self.registration.showNotification(title, { body: data.body || '', tag: data.tag || undefined, data: { url: data.url || '/calendar' }, icon: '/assets/icon.svg' }));
});
self.addEventListener('notificationclick', e => {
  e.notification.close();
  const url = (e.notification.data && e.notification.data.url) || '/calendar';
  e.waitUntil(self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then(cs => { const c = cs.find(c => 'focus' in c); if (c) { c.navigate(url); return c.focus(); } return self.clients.openWindow(url); }));
});
