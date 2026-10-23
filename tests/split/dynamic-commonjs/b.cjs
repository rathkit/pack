console.log('b runs');
module.exports = { name: 'b', shared: () => require('./shared.cjs').value + 1 };
