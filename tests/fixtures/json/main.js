import data from './data.json' with { type: 'json' };
import list from './list.json' with { type: 'json' };
console.log(data.name, data.nested.deep, list.length, list[1]);
