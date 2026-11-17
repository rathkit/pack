import { greet } from './greet.js';
import tiny from './tiny.cjs';
import banner from './banner.txt?raw';
import { label } from '@/label.js';

console.log(greet('world'), tiny(2), label, banner.trim(), __APP_VERSION__, import.meta.env.MODE, import.meta.env.VITE_API);
