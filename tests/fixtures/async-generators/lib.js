export function* gen() { yield 1; yield* [2, 3]; }
export const delay = (ms, v) => new Promise((r) => setTimeout(() => r(v), ms));
export async function run() { const gen = await delay(1, 'g'); return gen + (await delay(1, '!')); }
