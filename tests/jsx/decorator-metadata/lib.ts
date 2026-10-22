const store = new Map<object, Map<string | symbol | undefined, Map<string, unknown>>>();
(Reflect as any).metadata = (key: string, value: unknown) => (target: object, prop?: string | symbol) => {
  const byProp = store.get(target) ?? new Map();
  const entries = byProp.get(prop) ?? new Map();
  entries.set(key, value);
  byProp.set(prop, entries);
  store.set(target, byProp);
};
export const metadataOf = (target: object, prop?: string | symbol): Record<string, string> => {
  const entries = store.get(target)?.get(prop) ?? new Map();
  const show = (v: unknown): string => (Array.isArray(v) ? `[${v.map(show).join(',')}]` : typeof v === 'function' ? v.name : String(v));
  return Object.fromEntries([...entries].sort().map(([k, v]) => [k, show(v)]));
};
export const mark = (): any => () => {};
