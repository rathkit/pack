import { Ctx, version } from './shared.js';
import React from 'react';
const out = document.getElementById('out');
out.textContent = 'version ' + version + ' ctx ' + typeof Ctx.Provider + ' react ' + React.version;
document.title = 'done';
