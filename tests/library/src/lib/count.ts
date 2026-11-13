import { add } from './math';
export let calls = 0;
export function count(n: number): number {
  calls++;
  return add(n, 1);
}
