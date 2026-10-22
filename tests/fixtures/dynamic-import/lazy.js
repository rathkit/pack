import { shared, bump } from './shared.js';
import { depValue } from './dep.js';
console.log('lazy eval', shared, depValue);
bump();
export const value = 'lazy-' + shared;
export function fn() { return depValue + shared; }
export default class Lazy {}
