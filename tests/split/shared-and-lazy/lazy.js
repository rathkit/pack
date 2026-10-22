import { shared } from './shared.js';
export const value = 'lazy-value';
export function useShared() { return shared() + '!'; }
