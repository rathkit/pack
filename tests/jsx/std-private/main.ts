export {};
function log(label: string) {
  return (value: any, context: any) => {
    console.log('apply', label, context.kind, String(context.name), context.static, context.private);
    const wrap = (f: any) => function (this: any, ...a: any[]) { console.log('call', label, context.kind); return f.apply(this, a); };
    if (context.kind === 'getter' || context.kind === 'method') return wrap(value);
    if (context.kind === 'setter') return wrap(value);
    if (context.kind === 'accessor') {
      return { get: wrap(value.get), set: wrap(value.set), init(v: any) { console.log('init', label, v); return v; } };
    }
  };
}

class P {
  #stored = 1;
  @log('get') get #g() { return this.#stored; }
  @log('set') set #g(v: number) { this.#stored = v; }
  @log('acc') accessor #a = 10;
  @log('static acc') static accessor #sa = 20;
  @log('static get') static get #sg() { return 'sg'; }
  @log('meth') #m(n: number) { return n + 1; }
  @log('async') async #am() { return 'async'; }

  run() {
    this.#g = this.#g + 1;
    this.#a = this.#a + 5;
    return [this.#g, this.#a, P.#sa, P.#sg, this.#m(1), 'has' + (#a in this)].join();
  }
  later() { return this.#am(); }
}

const p = new P();
console.log(p.run());
p.later().then((v) => console.log(v));
