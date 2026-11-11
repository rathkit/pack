import React from 'react';
import { renderToString } from 'react-dom/server';
const App = () => React.createElement('ul', null, [1,2,3].map((i) => React.createElement('li', { key: i }, 'item ' + i)));
console.log(renderToString(React.createElement(App)));
