import type { Ext } from 'ext-pkg';
export interface ThingOptions { ext?: Ext; size: number }
export default class Thing {
  constructor(public options: ThingOptions) {}
  area(): number { return this.options.size ** 2; }
}
