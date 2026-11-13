// A second entry for a subpath export; it shares modules with the first.
import { count } from './lib/count';
export const extra = (n: number): number => count(n) + 1;
export { shout } from './lib/text';
