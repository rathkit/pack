function mark(target: any) { target.marked = true; }
function method(_target: any, key: string, descriptor: PropertyDescriptor) {
  const original = descriptor.value;
  descriptor.value = function (...args: any[]) { return key + ':' + original.apply(this, args); };
}
function field(target: any, key: string) { (target.constructor.fields ||= []).push(key); }
@mark
export class Svc {
  @field label = 'svc';
  @method run() { return 'ran'; }
}
@mark
export default class Dflt { @method hi() { return 'hi'; } }
export class Plain { value = 1; }
export enum Level { Low = 1, High }
export namespace Tools { export const twice = (n: number) => n * 2; }
