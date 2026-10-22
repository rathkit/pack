import type { Shape } from './types.ts';
import { area, type Unit, Circle } from './geometry.ts';
import defaultThing, { helper as h } from './util.ts';
import type { OnlyType } from './only-types.ts';

interface Local<T> { value: T; map<U>(f: (t: T) => U): Local<U> }
type Pair<A, B = A> = readonly [A, B] | { a: A; b?: B };
let maybe: string | undefined;
declare function ambient(x: number): void;

const shapes: Shape[] = [{ kind: 'sq', size: 3 } as Shape, { kind: 'sq', size: 4 } as Shape];
let counter!: number;
counter = shapes.length as number;
const total = shapes.reduce((sum: number, s: Shape): number => sum + area(s), 0);
const circle: Circle = new Circle(2);
const okay = (maybe ?? 'fallback')!;
function greet<T extends string = 'x'>(this: void, who: T, times?: number): `hi ${T}` {
  return `hi ${who}` as `hi ${T}`;
}
const arrow = async <T,>(x: T): Promise<T> => x;
const result = (await arrow<number>(5)) satisfies number;
abstract class Base<T> {
  abstract describe(): string;
  protected readonly id: number = 7;
  declare tag: T;
  static create<T>(this: new () => T): T { return new this(); }
  [key: string]: unknown;
  method?(): void;
}
class Impl extends Base<string> implements Local<string> {
  value = 'v';
  override describe(): string { return `impl ${this.id}`; }
  map<U>(f: (t: string) => U): Local<U> { return { value: f(this.value), map: null as never }; }
  overload(a: string): string;
  overload(a: number): number;
  overload(a: any): any { return a; }
}
function over(a: string): string;
function over(a: number): number;
function over(a: any) { return a; }
export { total as default };
console.log(total, counter, circle.area().toFixed(2), okay, greet('bob'), result, new Impl().describe(), new Impl().overload(1), over('s'), h(), defaultThing, typeof ambient);

// Arrow functions with annotations inside a conditional's branches.
const flag = counter > 0;
const pick = flag ? (c: string, i: number): number => c.length + i : null;
const other = !flag ? (c?: string): void => {} : (q: number): number => q * 2;
const nested = flag ? (flag ? 1 : 2) : 3;
console.log(pick?.('ab', 1), typeof other, other(4 as any), nested);
