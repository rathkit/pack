const tick = (v) => new Promise((r) => setTimeout(() => r(v), 2));
export const order = ['dep-start'];
export const ready = await tick('dep-ready');
order.push('dep-done');
export const later = async () => (await tick(1)) + 1;
