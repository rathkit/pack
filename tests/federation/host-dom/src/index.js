import React from 'react';
import { createRoot } from 'react-dom/client';
import App from 'domapp/App';

const out = document.getElementById('out');
const root = createRoot(out);
root.render(React.createElement(App, { label: 'rendered' }));
const check = () => {
  if (out.textContent) document.title = 'done';
  else setTimeout(check, 50);
};
check();
