import './augment';
export { default as Thing, type ThingOptions } from './thing';
export { default } from './thing';
export * from './consts';
import makeCache, { Cache } from './cache';
export const cache = makeCache<string>();
export type { Cache };
export { makeCache };
