import React from 'react';
export default function Button() {
  return 'button ' + React.version + ' same=' + (React === globalThis.__hostReact);
}
export const label = 'remote-label';
