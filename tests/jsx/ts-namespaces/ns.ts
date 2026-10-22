export namespace util {
  export const double = (n: number) => n * 2;
  export const constant = 42;
  export enum Kind { A = 'a' }
  export class Box<T> { constructor(private v: T) {} get() { return this.v; } }
}
export namespace Outer.Inner {
  export const deep = 'deep';
  export function fn() { return deep + '!'; }
}
export namespace Outer { export const shown = 'outer'; }
