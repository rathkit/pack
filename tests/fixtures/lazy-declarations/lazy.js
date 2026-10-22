export let a = 1, b;
export const c = [1, 2].map((x) => x + a);
export var d = 'v';
export class F {}
export class G { hi() { return 'G' + a; } }
export function h() { return a + c.length; }
const { x, y: [z] } = { x: 10, y: [20] };
export const sum = x + z;
export const obj = { x, z };
export const def = { k: 'default-obj' };
b = 'assigned later';
export { x as def2 };
export const [p, q = 5] = [1];
console.log('lazy ran', p, q);
