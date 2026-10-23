import { Color, Label, Level, Computed } from './colors';
import { Merged, Shape } from './merged';
import { Hue } from './reexport';
import * as ns from './colors';

enum Local {
  X = 2,
  Y = X * 2,
  Z = Local.Y + 1,
}

console.log(Color.Red, Color.Green, Color.Blue, Color.Neg, Color.Mask, Color.Mixed, Color['dash-ed']);
console.log(Label.Short, Label.Long, Label.Both, Label['Short']);
console.log(Level.Low, Level.High);
console.log(Computed.Known, Computed.Unknown, Computed.After);
console.log(Local.X, Local.Y, Local.Z);
console.log(Hue.Blue, Hue['Green']);
console.log(Merged.A, Merged.B, Shape.Square, Shape.sides);
console.log(Color.Green.toFixed(1), Color.Neg.toString(), Label.Short.length);
console.log(Color.Neg ** 2, -Color.Neg, typeof Color.Red, Color.Green in { 5: true });
console.log(ns.Color.Blue, ns.Label.Long);
console.log(Color[1], Color[Color.Green], Local[Local.X]);
console.log(Object.keys(Color).length, Object.keys(Label).join());
function inner(Color: { Red: string }) {
  return Color.Red;
}
console.log(inner({ Red: 'shadowed' }));

import lazy, { Palette, lazyValue } from './lazy';
console.log(new Palette().describe(), lazy, lazyValue());
const dynamic = await import('./lazy');
console.log(dynamic.Derived.Doubled, Object.keys(dynamic).sort().join());
