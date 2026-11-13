import './app.scss';
import fs from 'node:fs';
const css = fs.readFileSync(process.env.OUT_DIR + '/bundle.css', 'utf8').replace(/\s/g, '');

import { readdirSync } from 'node:fs';
const png = readdirSync(process.env.OUT_DIR).find((f) => /^pic-.*\.png$/.test(f));
console.log(!!png, css.includes('url(./' + png + ')'), css.includes('.bg{'));
