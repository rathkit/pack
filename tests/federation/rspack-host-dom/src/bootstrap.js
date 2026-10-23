import React from 'react';
import { createRoot } from 'react-dom/client';

const out = document.getElementById('out');
import('domapp/App').then(({ default: App }) => {
  createRoot(out).render(React.createElement(App, { label: 'rendered' }));
  const check = () => (out.textContent ? (document.title = 'done') : setTimeout(check, 50));
  check();
}).catch((e) => {
  out.textContent = 'ERROR ' + e.message;
  document.title = 'failed';
});
