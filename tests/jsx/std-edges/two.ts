import { note, mark } from './deco.ts';

const Item = 'shadow';

@mark
class Thing {
  @note('x') readonly value!: number;
  @note('g') get doubled(): number { return (this.value ?? 21) * 2; }
  static label = Item;
}

export { Thing, Item };

@mark
export default class {
  @note('anon') run(): string { return 'anonymous ran'; }
}
