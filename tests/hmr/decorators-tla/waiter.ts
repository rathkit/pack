export const ready: string = await new Promise((resolve) => setTimeout(() => resolve('ready'), 5));
export const value = ready + '!';
