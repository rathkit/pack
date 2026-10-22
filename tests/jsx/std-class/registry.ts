const names: string[] = [];

export function register(label: string) {
  return function <T extends new (...args: any[]) => any>(target: T, context: ClassDecoratorContext) {
    names.push(label + ':' + String(context.name));
    context.addInitializer(function () {
      names.push('init:' + label);
    });
    return class extends target {
      registered = label;
    };
  };
}

export function seal(target: any, context: ClassDecoratorContext) {
  console.log('seal', context.kind, String(context.name));
  Object.freeze(target.prototype);
}

export class Registry {
  static names() { return names; }
}
