export {};
// Computed names: keys and decorator expressions are evaluated once, in written order.
const order: string[] = [];
const key = (name: string) => { order.push('key ' + name); return name; };
const dec = (name: string) => {
  order.push('dec ' + name);
  return (value: any, context: any) => {
    console.log('apply', name, context.kind, String(context.name), context.static, context.private);
    if (context.kind === 'method') {
      return function (this: any, ...args: any[]) { console.log('call', name); return value.apply(this, args); };
    }
    if (context.kind === 'field') return (v: any) => v + '!';
    if (context.kind === 'getter') return function (this: any) { return 'got ' + value.call(this); };
  };
};
const sym = Symbol('s');

class K {
  @dec('a') [key('a')]() { return 'a'; }
  plain() { return 'plain'; }
  @dec('b') static [key('b')] = 'bee';
  [key('c')] = 'sea';
  @dec('d') get [key('d')]() { return 'dee'; }
  @dec('e') [sym]() { return 'sym'; }
  @dec('f') accessor [key('f')] = 'eff';
  accessor [key('g')] = 'gee';
  @dec('h') h = 'aitch';
  @dec('i') [key('i')];
}

console.log(order.join(','));
const k = new K() as any;
console.log(k.a(), k.plain(), K.b, k.c, k.d, k[sym](), k.f, k.g, k.h, k.i);
k.f = 'F';
console.log(k.f, Object.getOwnPropertyNames(K.prototype).join());
