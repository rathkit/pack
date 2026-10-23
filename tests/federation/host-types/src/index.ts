import makeButton, { version } from 'r3/Button';

const out = document.getElementById('out')!;
out.textContent = makeButton({ label: 'typed', size: version });
document.title = 'done';
