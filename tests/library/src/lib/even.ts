import { isOdd } from './odd';
// Two files that need each other.
export const isEven = (n: number): boolean => (n === 0 ? true : isOdd(n - 1));
