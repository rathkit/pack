import { of, from, Subject, BehaviorSubject, ReplaySubject, merge, concat, combineLatest, timer, interval, throwError, EMPTY, firstValueFrom, lastValueFrom } from 'rxjs';
import { map, filter, scan, mergeMap, switchMap, concatMap, take, takeUntil, debounceTime, distinctUntilChanged, catchError, retry, reduce, toArray, startWith, bufferCount, groupBy, delay, tap, finalize, share } from 'rxjs/operators';
import { ajax } from 'rxjs/ajax';
import { TestScheduler } from 'rxjs/testing';

const log = [];
const out = (label, value) => log.push(label + ' ' + JSON.stringify(value));
const collect = (source) => lastValueFrom(source.pipe(toArray()));

out('sync', await collect(of(1, 2, 3, 4, 5).pipe(map((x) => x * 2), filter((x) => x > 2), scan((a, b) => a + b, 0))));
out('merge', await collect(merge(of(1, 2), of(3)).pipe(take(3))));
out('concat', await collect(concat(of('a'), of('b', 'c'))));
out('combine', await collect(combineLatest([of(1, 2), of('x')]).pipe(map(([a, b]) => a + b))));
// The inner sources answer in the order 3, 2, 1, which is not the order they were subscribed in. They are opened by hand rather than by
// timers of 45, 30 and 15 ms: a runner that stalls for 15 ms between two subscriptions would reorder the timers and fail the comparison.
const gates = [new Subject(), new Subject(), new Subject()];
const merged = collect(from([1, 2, 3]).pipe(mergeMap((n) => gates[n - 1].pipe(take(1), map(() => n)))));
[3, 2, 1].forEach((n) => gates[n - 1].next(n));
out('mergeMap', await merged);
out('concatMap', await collect(from([1, 2, 3]).pipe(concatMap((n) => timer((4 - n) * 15).pipe(map(() => n))))));
out('switchMap', await collect(from([1, 2, 3]).pipe(switchMap((n) => of(n * 10)))));
out('group', await collect(from(['apple', 'avocado', 'banana', 'blueberry', 'cherry']).pipe(groupBy((s) => s[0]), mergeMap((g) => g.pipe(reduce((acc, s) => [...acc, s], [g.key]))))));
out('buffer', await collect(from([1, 2, 3, 4, 5]).pipe(bufferCount(2))));
out('error', await collect(throwError(() => new Error('boom')).pipe(catchError((e) => of('caught ' + e.message)))));
let attempts = 0;
out('retry', await collect(new (await import('rxjs')).Observable((s) => { attempts++; if (attempts < 3) s.error('no'); else { s.next('ok after ' + attempts); s.complete(); } }).pipe(retry(5))));
const subject = new Subject();
const seen = [];
subject.pipe(distinctUntilChanged(), tap((v) => seen.push(v))).subscribe();
[1, 1, 2, 2, 3].forEach((v) => subject.next(v));
out('subject', seen);
const replay = new ReplaySubject(2);
[1, 2, 3].forEach((v) => replay.next(v));
out('replay', await firstValueFrom(replay.pipe(toArray(), take(1), startWith('start'))).catch(() => 'pending'));
const behavior = new BehaviorSubject('initial');
out('behavior', behavior.value);
out('empty', await collect(EMPTY));
const done = [];
await lastValueFrom(of(1, 2).pipe(finalize(() => done.push('finalized')), share())).catch(() => {});
out('finalize', done);
const marbles = [];
new TestScheduler((a, b) => marbles.push(JSON.stringify(a) === JSON.stringify(b))).run(({ cold, expectObservable }) => {
  expectObservable(cold('a-b-c|', { a: 1, b: 2, c: 3 }).pipe(map((x) => x + 1))).toBe('a-b-c|', { a: 2, b: 3, c: 4 });
});
out('marbles', marbles);
out('ajax', typeof ajax);
console.log(log.join('\n'));
