import init from './a.wasm';
const { instance } = await init({});
console.log(instance.exports.add(2, 3));
