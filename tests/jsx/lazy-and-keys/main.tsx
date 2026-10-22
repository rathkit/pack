import React from 'react';
import { render } from './render.js';
import { Counter } from './counter.js';
console.log(render(<Counter start={5} />));
const lazy = await import('./lazy.js');
console.log(render(<lazy.Page title="lazy" />));
const props = { key: 'spread-key', a: 1 };
console.log(render(<div {...props} b={2} />));
console.log(render(<div key="k1" a="x" {...{ c: 3 }} />));
console.log(render(<p title="line one
line two">attr newline</p>));
console.log(render(<div>{'{'}braces{'}'} &nbsp;|&copy;|&unknown; > ok</div>));
