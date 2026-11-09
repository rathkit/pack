// The roadmap check, without the network: what counts as agreeing and what does not.
import assert from 'node:assert/strict';
import { items, compare } from './check-roadmap.mjs';

const text = [
  '- [x] 0.1 A done item with no issue is not checked.',
  '- [x] [1.1](https://github.com/o/r/issues/1) ticked and closed',
  '- [ ] [1.2](https://github.com/o/r/issues/2) open and not ticked',
  '- [x] [1.3](https://github.com/o/r/issues/3) ticked but open',
  '- [ ] [1.4](https://github.com/o/r/issues/4) closed but not ticked',
  '- [ ] [1.5](https://github.com/o/r/issues/99) missing',
  '- [ ] [1.6](https://github.com/o/r/issues/2) the same issue twice',
].join('\n');
const list = items(text);
assert.equal(list.length, 6);
assert.deepEqual(list[0], { ticked: true, id: '1.1', repo: 'o/r', issue: 1 });

const states = { 1: 'closed', 2: 'open', 3: 'open', 4: 'closed' };
const strict = compare(list, states);
assert.deepEqual(strict.problems, [
  '1.3: ticked, but issue #3 is open',
  '1.4: not ticked, but issue #4 is closed',
  '1.5: issue #99 was not found',
  '1.6: issue #2 is listed twice',
]);
const lenient = compare(list, states, true);
assert.deepEqual(lenient.warnings, ['1.3: ticked, but issue #3 is open']);
assert.equal(lenient.problems.length, 3);
assert.deepEqual(compare(list.slice(0, 2), states).problems, []);
// the project: an issue that is not an item of it is a problem, wherever its state; no project asked about means no such check
const onProject = new Set([1, 2, 3, 4]);
assert.deepEqual(compare(list.slice(0, 4), states, false, onProject).problems, ['1.3: ticked, but issue #3 is open', '1.4: not ticked, but issue #4 is closed']);
assert.deepEqual(compare(list.slice(0, 4), states, false, new Set([1, 2, 4])).problems, [
  '1.3: issue #3 is not on the project (https://github.com/orgs/rathkit/projects/1)',
  '1.3: ticked, but issue #3 is open',
  '1.4: not ticked, but issue #4 is closed',
]);
console.log('roadmap check: all cases agree');
