function F() { const get = () => new.target; return get() === F ? "new" : "call"; }
console.log(F(), new F() instanceof F, typeof new F());
class A { constructor() { this.t = (() => new.target === A || new.target === B)(); } }
class B extends A {}
console.log(new A().t, new B().t);
