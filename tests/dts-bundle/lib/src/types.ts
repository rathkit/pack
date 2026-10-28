import type { Ext } from 'ext-pkg';
export interface Options { name: string; extra?: Ext }
export type Mode = 'a' | 'b';
export interface Config<T = Options> { value: T; mode: Mode }
// Not exported, but the type of something that is: tsc writes `import("./types").Hidden`.
interface Hidden { secret: number }
export function makeHidden(): Hidden { return { secret: 1 }; }
export const defaults: Options = { name: 'x' };
// The same names are used in util.ts.
export interface Item { kind: 'types' }
export class Helper { types = true; }
