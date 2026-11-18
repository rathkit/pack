import { createSignal } from 'solid-js';
const [a] = createSignal(3);
const el = () => <div>{a()}</div>;
console.log(typeof el, a());
