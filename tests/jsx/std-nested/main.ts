export {};

function tag(label: string) {
  return (value: any, context: any) => {
    console.log('decorate', label, context.kind, String(context.name));
    if (context.kind === 'class') {
      return class extends value { tagged = label; };
    }
    if (context.kind === 'method') {
      return function (this: any, ...args: any[]) { console.log('call', label); return value.apply(this, args); };
    }
  };
}

function make(prefix: string) {
  @tag('decl')
  class Local {
    @tag('m') hello() { return prefix + ':local'; }
    accessor count = 3;
  }
  return Local;
}

const L = make('p');
const l = new L() as any;
console.log(l.hello(), l.tagged, l.count, L.name);

const Expr = (() => {
  return @tag('expr') class Named {
    @tag('e') who() { return 'named'; }
  };
})();
console.log(new Expr().who(), (new Expr() as any).tagged);

const anon = [@tag('anon') class { value = 1; }][0];
console.log(new anon().value, (new anon() as any).tagged);

{
  class Plain {
    accessor a = 1;
    static accessor b = 2;
  }
  console.log(new Plain().a, Plain.b);
}

class Outer {
  static inner() {
    class In {
      @tag('in') run() { return 'in'; }
    }
    return new In().run();
  }
}
console.log(Outer.inner());

// Names that anonymous class expressions get from where they stand.
let assigned: any;
assigned = @tag('assigned') class { };
const holder = {
  prop: @tag('prop') class { },
  plain: class { @tag('pm') m() { } },
};
function withDefault(P = @tag('param') class { }) { return P; }
class Fields {
  static slot = @tag('slot') class { };
  #hidden = class { accessor v = 1; };
  hidden() { return this.#hidden.name; }
}
const viaVar = class { accessor w = 2; };
console.log(assigned.name, holder.prop.name, holder.plain.name, withDefault().name, Fields.slot.name, new Fields().hidden(), viaVar.name);
console.log(JSON.stringify([@tag('free') class { }.name]));
