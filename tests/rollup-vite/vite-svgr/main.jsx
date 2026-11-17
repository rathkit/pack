import Logo from './a.svg?react';
import { renderToStaticMarkup } from 'react-dom/server';
console.log(renderToStaticMarkup(<Logo />).includes('<path'));
