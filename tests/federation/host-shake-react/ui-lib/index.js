import React from 'react';
export const makeLabel = () => 'lib ' + React.version + ' same=' + (React === globalThis.__hostReact);
export const unused = () => 'not read by the application';
