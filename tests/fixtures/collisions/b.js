const value = 'b'; function helper() { return '?'; } export { value }; export function run() { return 'b-run:' + value + helper(); }
