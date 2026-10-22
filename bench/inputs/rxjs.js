import { of, from, merge, interval, Subject } from 'rxjs';
import { map, filter, mergeMap, take, scan, debounceTime, catchError, switchMap } from 'rxjs/operators';
const s = new Subject();
merge(of(1, 2, 3), from([4, 5, 6])).pipe(filter((x) => x % 2 === 0), map((x) => x * 10), mergeMap((x) => of(x, x + 1)), scan((a, x) => a + x, 0), take(4), catchError(() => of(0)), switchMap((x) => of(x))).subscribe((v) => console.log('v', v));
s.pipe(debounceTime(1)).subscribe(() => {}); s.complete(); interval(1).pipe(take(1)).subscribe();
