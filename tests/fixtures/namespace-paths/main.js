import * as api from './api.js';
import { calls, report } from './pure.js';
// `api.math.double` names one export of a re-exported namespace; `api.text` is used whole.
console.log(api.math.double(21), Object.keys(api.text).sort().join(','));
console.log(typeof api.math.never, api.math.half(8));
console.log(report(), calls());
