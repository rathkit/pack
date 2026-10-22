export class Base { who() { return 'base2:' + Base.id; } static id = 2; static { Base.extra = Base.id * 10; } }
export function make() { const k = Base; return new k(); }
