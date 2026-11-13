import { shout } from './lib/text';
import { count } from './lib/count';
import data from './lib/data.json';
import legacy from 'cjs-dep';

export { shout, count };
export * from './lib/math';
export { isEven } from './lib/even';
export { isOdd } from './lib/odd';
export * as text from './lib/text';
export const version: string = data.version;
export const legacyName = legacy.name;
export type Pair<T> = [T, T];
export default function greet(name: string): string {
  return shout('hello ' + name);
}
