const table = { alpha: 1, beta: 'two' } as const;
export const A = 1 as const;
export const enum Dir { Up = 'up', Down = 'down' }
export function lookup<K extends keyof typeof table>(key: K): (typeof table)[K] { return table[key]; }
export type Table = typeof table;
export let counter = 0;
