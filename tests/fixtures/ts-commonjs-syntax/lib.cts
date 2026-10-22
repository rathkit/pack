namespace Inner { export const x = 'inner'; }
function add(a: number, b: number): number { return a + b; }
export = { add, Inner };
