import React from 'react';
import Logo from './logo.svg?react';
import { renderToStaticMarkup } from 'react-dom/server';
console.log(renderToStaticMarkup(React.createElement(Logo, { title: 'logo' })).includes('<circle'));
