import { note, mark } from './deco.ts';

@mark
export class Item {
  @note('f') declare?: string;
  @note('s') static count: number = 0;
  @note('m') describe(): string { return 'one:' + Item.name; }
  static make() { return new Item(); }
  static notes = (Item as any)[Symbol.metadata]?.notes.join('|');
}
