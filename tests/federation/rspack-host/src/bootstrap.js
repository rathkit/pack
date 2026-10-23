import React from 'react';
globalThis.__hostReact = React;
const out = document.getElementById('out');
out.textContent = 'react ' + React.version;
import('oapp/Button').then((m) => {
  out.textContent += ' | ' + m.default() + ' | ' + m.label;
  document.title = 'done';
}).catch((e) => {
  out.textContent += ' | ERROR ' + e.message;
  document.title = 'failed';
});
