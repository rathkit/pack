const log = (...a) => console.log(...a);
// Kept for what they do; the names are never read.
const unusedCall = log('call');
const unusedObject = { value: log('object') };
const unusedFunction = (function () { log('iife'); return 1; })();
const unusedClass = class { static s = log('class'); };
let unusedNew = new Map([[log('new'), 1]]);
var unusedVar = log('var');
export const exportedButUnused = log('exported');
// Read, so they stay as they are.
const used = log('used') ?? 'used-value';
export const alsoUsed = log('also') ?? 'also-value';
// Assigned later: not a plain unused binding.
let reassigned = log('reassigned');
reassigned = 2;
export default used;
