import React from 'react';
// Read when the module runs, not when something calls it: the chunk this is in is shared by the entry and by the exposed module.
export const Ctx = React.createContext('none');
export const version = React.version;
