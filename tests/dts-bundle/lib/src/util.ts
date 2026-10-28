import { Options, Mode, Config, Item as TypesItem } from './types';
import ExtDefault, { makeExt } from 'ext-pkg';
// Same names as in types.ts, but other things.
export interface Item { kind: 'util'; from: TypesItem }
export class Helper { util = true; }
export function describe(options: Options, mode: Mode = 'a'): string { return options.name + mode; }
export function configure(options: Options): Config { return { value: options, mode: 'a' }; }
export function extra() { return makeExt('1'); }
export const ext = new ExtDefault();
export abstract class Base<T> { abstract get(): T; value?: T; }
export class Impl extends Base<number> { get() { return 1; } }
export enum Color { Red, Green }
export function overload(a: string): string;
export function overload(a: number): number;
export function overload(a: any): any { return a; }
export const frozen = { a: 1, b: 'two' } as const;
export namespace Shapes { export interface Circle { r: number } export const unit: Circle = { r: 1 }; }
export interface Merged { a: number }
export declare namespace Merged { const extra: string }
