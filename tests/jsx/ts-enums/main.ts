import { Color, Flags, Mixed, Dir } from './enums.ts';
console.log(Color.Red, Color[Color.Green], Color.Blue, Flags.A | Flags.C, Flags.All, Mixed.S, Mixed.N, Mixed[0]);
console.log(Dir.Up, Dir.Down, Dir[2], Object.keys(Dir).join());
enum Local { X = 10, Y, Z = X * 2 + Y, W = 'w'.length, V = `t`, U = -1 }
console.log(Local.X, Local.Y, Local.Z, Local.W, Local.V, Local.U);
const enum Konst { A = 1, B = A << 3 }
console.log(Konst.A, Konst.B);
function inside() { enum Fn { P, Q = P + 5 } return [Fn.P, Fn.Q, Fn[6]]; }
console.log(inside());
enum Merge { A = 1 }
enum Merge { B = 2 }
console.log(Merge.A, Merge.B, Merge[2]);
let Color2 = Color; const Dir2 = Dir;
console.log(typeof Color2, Dir2.Left);
