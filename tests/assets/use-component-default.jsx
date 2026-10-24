import React from 'react';
import Logo from './logo.svg';
import { renderToStaticMarkup } from 'react-dom/server';
console.log(renderToStaticMarkup(React.createElement(Logo)).includes('<circle'));
