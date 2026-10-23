import Thing, { label } from 'inner/Thing';
const out = document.getElementById('out');
import('inner/Thing').then((m) => {
  out.textContent = Thing() + ' | ' + label + ' | dyn:' + m.default();
  document.title = 'done';
});
