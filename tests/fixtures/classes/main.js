import { Base, make } from './base.js';
import { Base as Base2, make as make2 } from './base2.js';
class Base3 extends Base { who() { return 'derived:' + super.who() + ':' + Base3.tag; } static tag = 'T'; }
console.log(new Base3().who(), make().who(), make2().who(), new Base2().who());
console.log(Base.name, Base2.name, Base3.name);
