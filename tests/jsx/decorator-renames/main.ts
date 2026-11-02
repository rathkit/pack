import { mark as first } from './one.js';
import { mark as second } from './two.js';

// Two modules export `mark`, so one of them is renamed in the bundle: a decorator on a field, a method or a parameter-less class
// must use the renamed name.
class Model {
  @first('a') count = 1;
  @second('b') name = 'x';
  @first('c') method() {}
}
console.log(new Model().count, Object.keys(new Model()).join());
