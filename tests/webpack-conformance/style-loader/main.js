// A page's document, as much as style-loader uses: a head to put a <style> tag in.
const styles = [];
global.window = global;
global.document = {
  head: { appendChild: (el) => styles.push(el) },
  querySelector: () => ({ appendChild: (el) => styles.push(el) }),
  createElement: () => ({ setAttribute() {}, appendChild() {}, styleSheet: null, textContent: '', childNodes: [], removeChild() {}, sheet: null }),
  createTextNode: (t) => ({ t }),
};
require('./a.css');
console.log('style tags', styles.length);
