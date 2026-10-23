import React from 'react';
// A container that is a module of the application. It is loaded to find out what the remotes share, before React is chosen, so
// it reads React when a module is asked for, not as it loads.
const modules = { './Thing': () => ({ __esModule: true, default: () => 'internal thing ' + React.version, label: 'internal-label' }) };
export const init = async () => {};
export const get = async (expose) => {
  if (!(expose in modules)) throw new Error('no ' + expose);
  return modules[expose];
};
