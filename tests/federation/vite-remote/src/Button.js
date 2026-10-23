import React from 'react';
export default function Button() {
  return 'vite button ' + React.version + ' same=' + (React === globalThis.__hostReact);
}
export const label = 'vite-label';
