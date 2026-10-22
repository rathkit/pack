import { a } from './lib.js'
let x = 1
x++
const y = [1, 2, 3]
;[4, 5].forEach(n => y.push(n))
console.log(a(), x, y)
const t = `tmpl ${x}`
console.log(t)
