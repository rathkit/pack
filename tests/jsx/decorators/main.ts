import { Base, replace as swap, registry } from './lib.js';

function log(target: any, key: string, desc: PropertyDescriptor) {
  const orig = desc.value;
  desc.value = function (this: any, ...a: any[]) { console.log('call', key, a); return orig.apply(this, a); };
  return desc;
}
function tag(name: string) {
  return (target: any, key?: string, desc?: any) => { console.log('tag', name, key ?? target.name, typeof desc); };
}
function sealed(ctor: Function) { console.log('sealed', ctor.name); }
function inject(token: string) {
  return (target: any, key: string | undefined, index: number) => { console.log('inject', token, key ?? 'constructor', index); };
}

@sealed
@tag('class')
class Service extends Base {
  @tag('prop') name = 'svc';
  @tag('static') static count = 0;
  constructor(@inject('A') private a: string, @inject('B') b: number) { super(); }
  @log
  @tag('method')
  run(@inject('X') x: number, y = 2) { return x + y + this.a.length; }
  @tag('getter') get value() { return 1; }
  @tag('static-method') static make() { return new Service('mk', 0); }
}
console.log(new Service('abc', 1).run(3), Service.make().value);

@swap
class Replaced { base = 'base'; }
console.log((new Replaced() as any).extra, (new Replaced() as any).base);

@tag('exported-later')
export class Exported { @tag('p') x = 1; }
export @tag('after-export') class AfterExport {}
@tag('default')
export default class DefaultOne { @tag('dp') y = 2; }
console.log(registry.length, new Exported().x, typeof AfterExport, new DefaultOne().y);
