import def, { named } from './plain.cjs';
import * as ns from './plain.cjs';
import esm, { flag } from './esmlike.cjs';
import { viaRequire } from './requirer.cjs';
console.log(def.kind, named, Object.keys(ns).includes('named'), ns.default === def);
console.log(esm, flag);
console.log(viaRequire);
