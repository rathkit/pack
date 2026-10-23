import React from 'react';
import { makeLabel } from 'ui-lib';
globalThis.__hostReact = React;
document.getElementById('out').textContent = makeLabel();
document.title = 'done';
