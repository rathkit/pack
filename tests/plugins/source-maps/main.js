// line 1
// line 2
function boom() {
  throw new Error('boom');
}
try {
  boom();
} catch (error) {
  const frame = error.stack.split('\n').find((l) => l.includes('main.js'));
  console.log(/main\.js:4:/.test(frame) ? 'maps to original line 4' : 'wrong: ' + frame);
}
