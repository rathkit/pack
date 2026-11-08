# Failures seen in CI that were not a bug in the bundler

What failed, what caused it, and what was done. The list covers the CI runs of this repository since 2026-10-30 (the repository
was recreated that day, so the logs of earlier runs are gone). A leg that fails for a reason on this list is re-run; a failure that
is not on it is a bug until shown otherwise.

| When | Leg | What failed | Cause | Status |
| --- | --- | --- | --- | --- |
| 2026-10-30, run 41093100446 | every Linux leg, arm64 included | the dev server HTTPS test and the HMR test: `ECONNREFUSED 127.0.0.1`, "the build was not served" | `localhost` resolved to `::1` first on the runners, and the server listened on only one of the two loopback addresses | Fixed in `d98cd9c`: `localhost` is served on both. Not seen since |
| 2026-10-31, run 41169248134 | `ubuntu-latest node 24` | the HMR test: `Cannot read properties of undefined (reading 'find')` in the removal check | A lookup in the removal check found no update message. The cause was not isolated | Made robust in `e2bb0a1` (the browser gets a second try, and the failure says why) and `9a49973` (the update and the log are printed) |
| 2026-10-31, run 41169248134 | `alpine (musl, arm64)` | the job did not start: JavaScript actions are not supported in Alpine containers on arm64 | A setting of the workflow, not a timing problem | Fixed in `efa0113`: the Alpine checks run from a script |
| 2026-11-02, run 41528201431 | `ubuntu-latest node 24` and `26` | the page checker test: `ENOTEMPTY` removing the browser's profile folder | Most likely the browser had just been stopped and was still writing to the folder | The folder is removed with retries (`tools/lib/browsers.mjs`) |
| 2026-11-05, run 42037557384 | `macos-15-intel node 24` | the HMR test, rxjs program: `mergeMap` printed `[1,3,2]` where `[3,2,1]` is expected | The program used three timers of 45, 30 and 15 ms. A slow runner that waits 15 ms between two subscriptions reorders them | Fixed: the inner sources are now opened by hand in the order 3, 2, 1, with no timer (`tests/hmr/soak/rxjs/main.js`). The HMR checks, rxjs program included, passed three times on an idle machine and once under a load of 9 on 16 threads |
| 2026-11-04, run 41857001294 | `ubuntu-24.04 node 26` | the job ran out of its 30 minutes | Installing Firefox and WebKit through Playwright took 24 minutes (a slow package mirror) | Each try of the install is cut off at 7 minutes and there are two (`.github/workflows/ci.yml`) |
| 2026-11-05, run 42144381689 | `ubuntu-24.04 node 26` | the first try of the install was cut off at 7 minutes, and the second failed with `Could not get lock /var/lib/dpkg/lock-frontend` | The mirror was slow again. The cut-off install left its `apt-get` running, and it held the lock | Between tries, the job now stops `apt-get` and `dpkg` and runs `dpkg --configure -a` |
| 2026-11-06, run 42233761814 | `ubuntu-24.04 node 26` | both tries of the install were cut off at 7 minutes (the dpkg lock was free this time) | A package of 5.6 MB stalled for four and a half minutes before apt gave up on it, twice | apt now gives up on a silent connection after 15 seconds and retries five times (`.github/workflows/ci.yml`) |
| 2026-11-06, run 42289534456 | `ubuntu-24.04 node 26` | both tries of the install were cut off at 7 minutes, with no download output for the whole of the second | apt hung on the mirror, and its 15-second limit did not help | The downloaded packages are kept in the Actions cache (`apt-debs-playwright-1.64.1-v1`); a run that finds them does not use the mirror for them |
| 2026-11-06, run 42304447689 | `macos-15-intel node 24` | `rustup toolchain install` stopped: `could not download file from 'https://static.rust-lang.org/dist/channel-rust-stable.toml.sha256'` | The Rust download server did not answer within 30 seconds | The install is tried up to three times (`.github/workflows/ci.yml`) |
| 2026-11-06, run 42304447689 | `macos-26-intel node 24` | the fixture battery (`bundled_programs_behave_like_the_originals`) was not over in 300 seconds and was killed, with nothing printed | Not known. The battery takes 18 seconds on a Linux machine and the other Intel legs finished the whole test step in 3 to 5 minutes, so a hang or a very overloaded runner is more likely than a slow one | The limit is 600 seconds, each fixture prints its time as it ends, and a bundler run that takes more than 120 seconds fails with the fixture's name. If it happens again the log will show where it stopped |
| 2026-11-07, run 42325652339 | `macos-26-intel node 24` | the source map check (`tools/run-sourcemaps.mjs`) printed nothing for 46 minutes and the job was cancelled at its 45-minute limit | Not known. Every earlier step took seconds and the script takes about a minute elsewhere, so one program it started (the bundler or Node) did not end. The same leg and Node version also lost the fixture battery in the previous row | Each program the script starts now has a 120-second limit and fails with its name and the reason, so a hang costs two minutes. If it happens again the log will name the program |
| 2026-11-07, run 42332724580 | `macos-15 node 26` (a required leg) | `cargo test --release`: `estree_matches_acorn` "has been running for over 60 seconds" at 03:19 and never ended; the job was cancelled at its 45-minute limit. Two processes were left behind, `rkp` and `node` | Not known. The script (`tools/check-estree.mjs`) pipes each file to `rkp --estree -` and waits with no limit, so either `rkp` or the pipe stopped on one file. Nothing like it has happened on the other 13 runs of this leg | The test runs under the 600-second limit that kills what it started, each `rkp` call in the script has a 60-second limit and a hang names its file, and the `cargo test` step has a 20-minute limit so its log is kept |
| 2026-11-07, run 42347747839 | `alpine (musl) node 24` | `docker: Error response from daemon: received unexpected HTTP status: 502 Bad Gateway` while pulling `node:24-alpine`, six seconds into the job | The Docker registry answered 502 | `tools/ci-musl.sh` pulls the image up to three times, 15 seconds apart |
| 2026-11-07, run 42347747839 | `macos-26-intel node 24` | `estree_matches_acorn`: `rkp --estree -` on `tests/plugins/webpack-compiler/env.prod.js` got no answer in 60 seconds (the new limit named the file); 378 of 379 files had agreed | Not known, and not the file: it has 33 bytes and 300 runs of the same command on Linux all end at once. This is the third hang on this leg and Node version in two days (fixture battery, source maps, now this), always where a child process is started, and none on any other leg | None yet. The limits make each one fail in a minute or two with a name instead of holding the job |
| 2026-11-04, run 41757108691 | `macos-26-intel node 24` | the `pnpm/setup` step failed | A DNS lookup failed on the runner | Not ours. Re-run passed |
| 2026-11-03 to 2026-11-05, for example runs 41857001294 and 42054104725 | macOS legs (`macos-15`, `macos-15-intel`, `macos-26`, `macos-26-intel`) | the job was cancelled before any step: "The job was not acquired by Runner of type hosted even after multiple attempts" | GitHub had no macOS runner free. Its annotation says macOS arm64 capacity is limited | Not ours. The failed jobs are re-run, and sometimes more than once |

## The declaration-file timeout on musl

The roadmap named a timeout of the declaration-file (`--dts`) test on musl. No such failure is in the logs that still exist. Today the
whole fixture suite, which includes that test, finishes in 33 to 48 seconds on the four Linux legs checked (run 42095264960), and each
script has a limit of 300 seconds (`SCRIPT_LIMIT` in `tests/fixtures.rs`). It was not reproduced, so nothing was changed. If it
comes back, the log of the run goes into this file with its cause.

## What to do when a leg fails

1. Read the annotation of the job. "Not acquired by Runner", a DNS error or a download that timed out is the runner, not the code.
2. For those, re-run the failed jobs. Do not push another commit to get a fresh run: a push to `main` cancels the run in progress.
3. For anything else, treat it as a bug in the code or in the test, find the cause, and add a line here when it turns out to be a
   timing or environment problem.

## Which legs block a merge, and what counts as a clean run

A check that must be green before a change merges is a check that can be red for a reason that is not the change. Runner capacity
failed macOS legs on 2026-11-03, 11-04 and 11-05, on all four macOS images. So the legs are split:

| Legs | In the main ruleset |
| --- | --- |
| `ubuntu-24.04`, `ubuntu-24.04-arm` and the four `alpine (musl…)` legs, Node 24 and 26 | Required (already) |
| `windows-2025`, `windows-2022` and `windows-11-arm`, Node 24 and 26 (six legs) | Required |
| `macos-15`, Node 24 and 26 (Apple silicon) | Required |
| `macos-15-intel`, `macos-26` and `macos-26-intel`, Node 24 and 26 (six legs) | Advisory: they run on every push and show red, but do not block |
| `ubuntu-26.04` and `ubuntu-26.04-arm`, Node 24 and 26 (four legs) | Advisory, as before |

A run is **clean** when every required job that got a runner passed on its first attempt. A job that never got a runner has no result:
it is re-run, and the run goes on this page with the date. It does not count against the run. A required job that started and failed does,
whatever the cause. An advisory leg that fails does not break the run, but the failure still goes on this page with the date, so a
leg that keeps failing is seen. The required legs were added to the ruleset on 2026-11-06. Roadmap item 1.2 is done after ten clean runs
in a row with them required.

A run started by hand on `main` (`gh workflow run ci -r main`) counts, and the same commit may be run again: the point of the ten is to see
whether the tests are flaky. Start them one after another, not together: macOS runners are limited, and a run that is waiting for one
is a run that did not get a runner.
