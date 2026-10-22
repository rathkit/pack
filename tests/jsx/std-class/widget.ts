import { register, seal } from './registry.ts';

class Base {
  base = 'base';
}

@register('a')
@seal
class Mid extends Base {
  mid = 'mid';
  static kind = 'mid';
}

@register('w')
export default class Widget extends Mid {
  hello() {
    return [this.base, this.mid, (this as any).registered, Widget.name, Mid.kind].join('/');
  }
}
