import { util, Outer } from './ns.ts';
console.log(util.double(4), util.constant, util.Kind.A, new util.Box(3).get());
console.log(Outer.Inner.deep, Outer.Inner.fn(), Outer.shown);
namespace Local {
  export let counter = 1;
  export function bump() { return ++counter; }
  const hidden = 'h';
  export const reveal = () => hidden + counter;
  export namespace Nested { export const v = 'nested'; }
  export enum E { One = 1 }
}
console.log(Local.bump(), Local.reveal(), Local.Nested.v, Local.E.One);
namespace Local { export const added = 'merged'; }
console.log(Local.added, Local.bump());
class Merged { static base = 1 }
namespace Merged { export const extra = 2 }
console.log(Merged.base, Merged.extra);
namespace TypesOnly { export interface I { a: 1 } export type T = string }
console.log(typeof (globalThis as any).TypesOnly);
