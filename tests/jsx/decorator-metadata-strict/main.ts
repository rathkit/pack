import { metadataOf, mark } from './lib.js';
import { Dep, type Shape, Level } from './dep.js';
import * as ns from './dep.js';

class Local {}

@mark()
class Service {
  @mark() prop!: Dep;
  @mark() text = 'x';
  @mark() list!: string[];
  @mark() maybe!: string | null;
  @mark() many!: Dep | Local;
  @mark() iface!: Shape;
  @mark() level!: Level;
  @mark() viaNs!: ns.Dep;
  @mark() fn!: () => void;
  @mark() local!: Local;
  @mark() lit!: 'a';
  @mark() tuple!: [number, string];
  @mark() big!: bigint;
  @mark() sym!: symbol;
  @mark() any!: any;
  @mark() ro!: readonly string[];
  @mark() generic!: Promise<Dep>;
  constructor(a: Dep, @mark() b: number, c?: string | null, d: boolean = false, ...rest: string[]) {}
  @mark() method(@mark() x: number, y: Dep, z: Local): string { return ''; }
  @mark() async am(): Promise<string> { return ''; }
  @mark() get acc(): boolean { return true; }
  set acc(v: boolean) {}
  @mark() noReturn(x: Dep[]) {}
  @mark() static make(a: Shape): void {}
}
const keys = ['prop', 'text', 'list', 'maybe', 'many', 'iface', 'level', 'viaNs', 'fn', 'local', 'lit', 'tuple', 'big', 'sym', 'any', 'ro', 'generic', 'method', 'am', 'acc', 'noReturn'];
console.log('class', JSON.stringify(metadataOf(Service)));
for (const k of keys) console.log(k, JSON.stringify(metadataOf(Service.prototype, k)));
console.log('static make', JSON.stringify(metadataOf(Service, 'make')));
