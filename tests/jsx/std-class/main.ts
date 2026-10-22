// Class decorators: replacing the class, adding initializers, stacking, derived classes, default export.
import Widget from './widget.ts';
import { Registry } from './registry.ts';

console.log(Widget.name, new Widget().hello());
console.log(Registry.names().join());
