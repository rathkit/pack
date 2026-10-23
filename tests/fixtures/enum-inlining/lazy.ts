import { Color, Label } from './colors';

export class Palette {
  static first = Color.Green;
  pick = Color.Blue;
  label: string = Label.Short;
  constructor(public fallback = Color.Mask) {}
  describe(level = Color.Neg) {
    return [this.pick, this.label, this.fallback, level, Palette.first].join('/');
  }
}

export enum Derived {
  FromOther = 4,
  Doubled = FromOther * 2,
}

export default { red: Color.Red, derived: Derived.Doubled, nested: [Label.Short, [Color.Mixed]] };

export const lazyValue = () => `${Color.Red}-${Derived.FromOther}`;
