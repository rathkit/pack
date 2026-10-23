import React from 'react';
globalThis.__hostReact = React;
const out = document.getElementById('out');
const parts = [];
// The same remote by its script address, and by code that gives its container; another by SystemJS.
Promise.all([import('oapp/Button'), import('viaPromise/Button'), import('sapp/Button')])
  .then(([byScript, byPromise, bySystem]) => {
    parts.push('script ' + byScript.default(), 'promise ' + byPromise.default(), 'system ' + bySystem.default());
    out.textContent = parts.join(' | ');
    document.title = 'done';
  })
  .catch((e) => {
    out.textContent = 'ERROR ' + e.message;
    document.title = 'failed';
  });
