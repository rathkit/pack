export class Cache<V> {
  private store = new Map<string, V>();
  get(key: string): V | undefined { return this.store.get(key); }
  set(key: string, value: V): this { this.store.set(key, value); return this; }
}
export default function makeCache<V>(): Cache<V> { return new Cache<V>(); }
