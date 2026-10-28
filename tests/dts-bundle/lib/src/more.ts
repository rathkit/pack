import { describe, Item } from './util';
export const more = describe({ name: 'm' });
export type MoreItem = Item;
export default function main(item: Item): MoreItem { return item; }
export * as util from './util';
