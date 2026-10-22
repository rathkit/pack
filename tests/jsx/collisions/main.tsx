import React, { Fragment } from 'react';
import { render } from './render.js';
import { App as InnerApp, jsx as helperJsx } from './inner.js';
import { Widget } from './widget.js';
const App = () => <main><InnerApp label="outer" /><Widget size={3} /></main>;
const jsx = 'shadow';
const _jsx = 'shadow2';
function Local({ jsx: j }: { jsx: string }) { return <em>{j}{jsx}{_jsx}</em>; }
console.log(render(<App />));
console.log(render(<Local jsx="prop" />));
console.log(render(<Fragment key="f"><b>x</b></Fragment>));
console.log(render(<React.Fragment><u>1</u><u>2</u></React.Fragment>));
console.log(helperJsx);
