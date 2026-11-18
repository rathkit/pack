import { greet } from './greet.js';
import tiny from './tiny.cjs';
import banner from './banner.txt';
import { label } from '@/label.js';

console.log(greet('world'), tiny(2), label, banner.trim(), __APP_VERSION__, process.env.NODE_ENV);
