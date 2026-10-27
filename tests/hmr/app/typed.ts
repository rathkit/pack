export enum Color { Red, Green, Blue }
export interface Item { id: number }
export function describe<T extends Item>(item: T): string { return `item#${item.id}`; }
export class Box<T> { constructor(private value: T) {} get(): T { return this.value; } }
export namespace Util { export const twice = (n: number) => n * 2; export function name() { return 'util'; } }
