// Field and accessor decorators: initializer replacement, ordering, statics, auto-accessors.
export {};
function twice(_: undefined, context: ClassFieldDecoratorContext) {
  console.log('field', String(context.name), context.static);
  return (v: number) => v * 2;
}

function trace(target: any, context: ClassAccessorDecoratorContext) {
  console.log('accessor', String(context.name));
  return {
    get(this: any) { const v = target.get.call(this); console.log('read', String(context.name), v); return v; },
    set(this: any, v: any) { console.log('write', String(context.name), v); target.set.call(this, v); },
    init(v: any) { console.log('init', String(context.name), v); return v; },
  };
}

function after(name: string) {
  return (_t: any, context: any) => {
    context.addInitializer(function (this: any) { console.log('extra', name); });
  };
}

class Thing {
  plain = (console.log('plain'), 1);
  @twice a = 10;
  @twice static s = 3;
  @trace accessor x = 5;
  accessor y = 6;
  static accessor z = 7;
  @after('m') m() { return 'm'; }
  @twice b = 20;
  @after('late') late = 1;
  other = (console.log('other'), 2);
  constructor(public tag: string) {
    console.log('ctor body');
  }
}

const t = new Thing('t');
console.log(t.a, t.b, Thing.s, t.x, t.y, Thing.z, t.plain, t.late, t.other);
t.x = 9;
t.y = 8;
console.log(t.x, t.y);

class Derived extends Thing {
  @twice d = 4;
  constructor() {
    super('d');
    console.log('derived body');
  }
}
console.log(new Derived().d);
