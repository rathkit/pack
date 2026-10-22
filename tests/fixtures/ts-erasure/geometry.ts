import type { Shape, } from './types.ts';
export type { Unit } from './types.ts';
export function area(s: Shape): number { return s.size * s.size; }
export class Circle {
  r: number; constructor(r: number) { this.r = r; } area() { return Math.PI * this.r ** 2; }
}
