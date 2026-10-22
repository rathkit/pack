// The page side of `rathkit-pack serve`: reloads when the build changes, swaps style sheets without a reload,
// shows build errors, and gives code written for Vite its `import.meta.hot` (see src/serve.rs for its limits).
const listeners = new Map();
const disposers = [];
let overlay = null;

const showError = (message) => {
  hideError();
  overlay = document.createElement('div');
  overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:rgba(10,10,10,.92);color:#ff8f8f;font:14px/1.5 ui-monospace,monospace;padding:32px;overflow:auto;white-space:pre-wrap';
  overlay.textContent = 'Build error\n\n' + message;
  overlay.addEventListener('click', hideError);
  document.body.appendChild(overlay);
};
const hideError = () => {
  overlay?.remove();
  overlay = null;
};
const reload = () => {
  for (const dispose of disposers.splice(0)) {
    try {
      dispose({});
    } catch {}
  }
  location.reload();
};
const swapStyles = (files, t) => {
  for (const file of files) {
    const link = [...document.querySelectorAll('link[rel="stylesheet"]')].find((l) => new URL(l.href, location.href).pathname === '/' + file);
    if (!link) return reload();
    const next = link.cloneNode();
    next.href = `/${file}?t=${t}`;
    next.addEventListener('load', () => link.remove());
    next.addEventListener('error', () => next.remove());
    link.after(next);
  }
};

const source = new EventSource('/__rathkit/events');
let opened = false;
source.onmessage = (event) => {
  const message = JSON.parse(event.data);
  switch (message.type) {
    case 'connected':
      // A second "connected" means the server restarted: the page may be out of date.
      if (opened) reload();
      opened = true;
      break;
    case 'full-reload':
      hideError();
      reload();
      break;
    case 'css-update':
      hideError();
      swapStyles(message.files, message.t);
      break;
    case 'error':
      showError(message.message ?? message.err?.message ?? 'Build failed');
      break;
    case 'custom':
      for (const fn of listeners.get(message.event) ?? []) fn(message.data);
      break;
    default:
      break;
  }
};

globalThis.__rathkit_hot = (id) => ({
  data: {},
  accept() {},
  acceptExports() {},
  dispose(callback) {
    disposers.push(callback);
  },
  prune() {},
  decline() {},
  invalidate() {
    reload();
  },
  on(event, callback) {
    if (!listeners.has(event)) listeners.set(event, []);
    listeners.get(event).push(callback);
  },
  off(event, callback) {
    listeners.set(event, (listeners.get(event) ?? []).filter((f) => f !== callback));
  },
  send(event, data) {
    fetch('/__rathkit/send', { method: 'POST', body: JSON.stringify({ event, data }) });
  },
});
