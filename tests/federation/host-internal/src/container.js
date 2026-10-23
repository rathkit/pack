import React from 'react';
// A container that is a module of the application, reading React as it loads: the runtime has loaded React before it loads this.
const version = React.version;
const modules = { './Thing': () => ({ __esModule: true, default: () => 'internal thing ' + version, label: 'internal-label' }) };
export const init = async () => {};
export const get = async (expose) => {
  if (!(expose in modules)) throw new Error('no ' + expose);
  return modules[expose];
};
