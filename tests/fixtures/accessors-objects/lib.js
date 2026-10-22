let hidden = 1;
export const box = { get val() { return hidden; }, set val(v) { hidden = v * 2; } };
export class Counter { n = 0; next() { this.n++; return this; } }
