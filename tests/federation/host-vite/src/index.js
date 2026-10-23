import React from 'react';
import Button, { label } from 'vapp/Button';
globalThis.__hostReact = React;
const out = document.getElementById('out');
out.textContent = ['react ' + React.version, Button(), label].join(' | ');
import('vapp/Button').then((m) => {
  out.textContent += ' | dyn:' + m.default();
  document.title = 'done';
}).catch((e) => {
  out.textContent += ' | ERROR ' + e.message;
  document.title = 'failed';
});
