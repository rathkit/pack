export class Base { baseProp = 1; }
export const registry: string[] = [];
export function replace<T extends new (...a: any[]) => {}>(c: T) {
  registry.push(c.name);
  return class extends c { extra = 'added'; };
}
