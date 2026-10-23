import map from 'lodash-es/map.js';
import { capitalize } from 'lodash-es';
export const double = (n) => n * 2;
export const shout = (text) => capitalize(text) + '!';
export const doubled = (list) => map(list, double);
export default 'utils-default';
