const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;');
export function render(node) {
  if (node == null || node === false || node === true) return '';
  if (Array.isArray(node)) return node.map(render).join('');
  if (typeof node !== 'object') return esc(node);
  const { type, props, key } = node;
  if (typeof type === 'function') return render(type(props));
  const inner = render(props.children);
  if (type === Symbol.for('fragment')) return inner;
  const attrs = Object.entries(props).filter(([k]) => k !== 'children').map(([k, v]) => ` ${k}=${JSON.stringify(v)}`).join('');
  return `<${type}${key !== null ? ` key=${JSON.stringify(key)}` : ''}${attrs}>${inner}</${type}>`;
}
