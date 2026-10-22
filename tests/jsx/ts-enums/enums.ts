export enum Color { Red, Green, Blue }
export enum Flags { None = 0, A = 1 << 0, B = 1 << 1, C = 1 << 2, All = A | B | C }
export enum Mixed { N = 1, S = 'str' }
export const enum Dir { Up = 1, Down, Left = 'left' }
