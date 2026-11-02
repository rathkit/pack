export function mark(name: string) {
  return (target: any, key: string) => { console.log('two', name, key); };
}
