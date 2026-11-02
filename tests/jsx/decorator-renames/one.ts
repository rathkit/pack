export function mark(name: string) {
  return (target: any, key: string) => { console.log('one', name, key); };
}
