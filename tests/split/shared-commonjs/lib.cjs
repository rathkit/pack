console.log('lib.cjs eval');
let count = 0;
module.exports = { id: () => ++count };
module.exports.named = 'N';
