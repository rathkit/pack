import map from 'lodash-es/map.js';
import { capitalize } from 'lodash-es';
import React from 'react';
import { double, shout, doubled } from 'r2/utils';
import utils from 'r2/utils';
const out = document.getElementById('out');
const lines = [
  'map ' + map([1, 2], double).join(),
  'shout ' + shout('hi'),
  'doubled ' + doubled([3]).join(),
  'default ' + utils,
  'capitalize ' + capitalize('x'),
  'react ' + React.version,
];
Promise.all([import('r2/Old'), import('r2/Gone')])
  .then(([old, gone]) => {
    lines.push('old ' + old.shout('a'), 'gone ' + gone.default, 'loaded ' + (globalThis.__loaded || []).join(','));
    const copies = Object.values(globalThis.__FEDERATION__.__INSTANCES__[0].shareScopeMap.default['lodash-es'] || {}).length;
    lines.push('lodash copies ' + copies);
    out.textContent = lines.join(' | ');
    document.title = 'done';
  })
  .catch((e) => {
    out.textContent = lines.join(' | ') + ' | ERROR ' + e.message;
    document.title = 'failed';
  });
