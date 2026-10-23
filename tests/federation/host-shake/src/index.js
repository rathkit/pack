import { map } from 'lodash-es';
const out = document.getElementById('out');
out.textContent = 'mapped ' + map([1, 2, 3], (n) => n * 2).join();
document.title = 'done';
