export class Base { who() { return 'base1:' + Base.id; } static id = 1; static #p = 5; static peek() { return Base.#p; } }
export function make() { return new Base(); }
