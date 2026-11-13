import { isEven } from './even';
export const isOdd = (n: number): boolean => (n === 0 ? false : isEven(n - 1));
