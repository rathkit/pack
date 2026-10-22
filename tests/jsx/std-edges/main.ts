import { Item } from './one.ts';
import Anon, { Thing, Item as ItemName } from './two.ts';

console.log(Item.make().describe(), Item.notes, Item.name);
console.log(new Thing().doubled, Thing.label, ItemName, Thing.name);
console.log(new Anon().run(), Anon.name.startsWith('default'));
const meta = (Anon as any)[Symbol.metadata];
console.log(meta && meta.notes.join());
