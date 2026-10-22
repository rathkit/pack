// Method, getter, setter, static and private-method decorators with context objects.
export {};
function log(label: string) {
  return function (target: any, context: any) {
    console.log('decorate', label, context.kind, String(context.name), context.static, context.private);
    if (context.kind === 'method') {
      return function (this: any, ...args: any[]) {
        console.log('call', label, args.join());
        return target.apply(this, args);
      };
    }
    if (context.kind === 'getter') {
      return function (this: any) { console.log('get', label); return target.call(this); };
    }
    if (context.kind === 'setter') {
      return function (this: any, v: any) { console.log('set', label, v); target.call(this, v); };
    }
  };
}

function tag(name: string) {
  return (_t: any, context: any) => {
    context.addInitializer(function (this: any) {
      console.log('init', name, typeof this);
    });
  };
}

class Calc {
  #total = 0;

  @log('add') @tag('add')
  add(a: number, b: number) { return a + b; }

  @log('sq')
  static square(n: number) { return n * n; }

  @log('priv')
  #secret(n: number) { return n + 100; }

  reveal(n: number) { return this.#secret(n); }

  @log('total')
  get total() { return this.#total; }

  @log('total')
  set total(v: number) { this.#total = v; }
}

const c = new Calc();
console.log(c.add(1, 2));
console.log(Calc.square(4));
console.log(c.reveal(5));
c.total = 7;
console.log(c.total);
console.log(Object.getOwnPropertyNames(Calc.prototype).sort().join());
