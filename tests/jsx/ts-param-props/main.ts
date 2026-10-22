class Base { constructor(public a: number, protected b = 2) { console.log('base', a, b); } }
class Derived extends Base {
  extra = 'x';
  constructor(public readonly c: string, private d?: number, a = 5) {
    super(a);
    console.log('derived', this.c, this.d, this.extra);
  }
  show() { return [this.a, (this as any).b, this.c, this.d]; }
}
console.log(new Derived('see', 4).show());
class Plain { constructor(readonly only: string) {} }
console.log(new Plain('p').only, Object.keys(new Plain('q')));
