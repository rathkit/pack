import { counter, bump, label as lbl } from './lib.js'
import * as lib from './lib.js'
import def from './lib.js'

// Shadowing at every level.
const value = 10
function shadow(value, other = value + 1) {
  { let value = other * 2; if (value > 0) { var kept = value } }
  return [value, other, kept]
}
console.log(shadow(1), shadow(2, 5))

// A parameter list with defaults and a body that declares its own names.
function defaults(a, b = () => a, c = a + 1) {
  var d = 4
  let e = 5
  return [a, b(), c, d, e]
}
console.log(defaults(1))

// Closures in loops capture each iteration.
const fns = []
for (let index = 0; index < 3; index++) fns.push(() => index * 10)
for (const item of [7, 8]) fns.push(() => item)
console.log(fns.map((f) => f()))

// Catch parameters, labels, switch and comma expressions.
function control(input) {
  outer: for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      if (i * j === input) break outer
      if (j > i) continue outer
    }
  }
  try { throw new Error('boom ' + input) } catch (error) { var caught = error.message }
  switch (input) { case 1: { let scoped = 'one'; return [scoped, caught] } default: return [caught] }
}
console.log(control(1), control(4))

// Destructuring with defaults and renames, shorthand properties, computed keys.
const { x = 1, y: renamed = 2, ...rest } = { z: 3, w: 4 }
const [first, , third = 'c', ...others] = [1, 2, undefined, 4, 5]
const key = 'dyn'
const object = { x, renamed, first, [key]: others, method() { return this.x }, get g() { return renamed }, async *gen() { yield x } }
console.log(x, renamed, rest, first, third, others, object.method(), object.g, Object.keys(object))

// Classes: inner name, static members, fields, accessors, private names, inheritance.
class Base {
  static count = 0
  #secret = 42
  field = Base.count++
  get secret() { return this.#secret }
  static create() { return new this() }
}
class Derived extends Base {
  extra = this.field + 100
  describe() { return [this.secret, this.extra, Derived.count] }
}
console.log(Derived.create().describe())

// Generators, async functions, iterators, tagged templates.
function* numbers(limit) { for (let n = 0; n < limit; n++) yield n }
const tag = (strings, ...values) => strings.raw.join('|') + values.join(',')
const total = [...numbers(4)].reduce((sum, n) => sum + n, 0)
console.log(total, tag`a${1}b${2}c`, `${total}${'x'.repeat(2)}`)
await Promise.all([1, 2].map(async (n) => n * 2)).then((v) => console.log(v))

// Regular expressions next to division, and operators that must not merge.
let a = 8, b = 2, g = 1
console.log(a / b / g, /ab+c/g.test('xabbc'), a / b + +'1' - -'2', a - --b, a+ +b, 1 .toFixed(1), 2..toFixed(1))

// Statements that rely on a line break instead of a semicolon.
let n = 0
n
++n
const r = n
/2/g
console.log(n, r)
function early() {
  return
  1
}
console.log(early())

// Imports, namespace, default, live bindings.
bump()
console.log(counter, lbl, Object.keys(lib).sort(), def(2), lib.counter)

// Uses of globals that must not be captured by new names.
function globals(p, q, r2, s, t, u, v, w) { return [p, q, r2, s, t, u, v, w].length + Math.max(p, q) + (typeof globalThis) + JSON.stringify({ NaN, Infinity, undefined }) }
console.log(globals(1, 2, 3, 4, 5, 6, 7, 8))
