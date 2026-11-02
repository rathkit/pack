// Types re-exported by name, as a library's entry point does: they are gone from the output.
export { Draft, Patch, value } from './internal';
import { value } from './internal';

export function outer() {
	// A nested overload signature leaves nothing behind either.
	function f<T>(obj: T): T
	function f(obj: any) {
		return obj
	}
	return f(value())
}
console.log(outer());
