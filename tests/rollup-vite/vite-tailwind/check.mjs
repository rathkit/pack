import { readdirSync, readFileSync } from 'node:fs';
export default (out) => (readdirSync(out, { recursive: true }).some((f) => f.endsWith('.css') && readFileSync(out + '/' + f, 'utf8').includes('tailwindcss')) ? null : 'no tailwind css in the output');
