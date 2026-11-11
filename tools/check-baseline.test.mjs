import assert from "node:assert/strict";
import { test } from "node:test";
import { compareAll, compareCell, DEFAULT_TOLERANCE, median, parseMaxRss, renderTable } from "./check-baseline.mjs";

const was = { bytes: 100000, gzip: 30000, rssKb: 50000 };

test("the peak memory is the last line of what time wrote, when it is a number", () => {
  assert.equal(parseMaxRss("123456\n"), 123456);
  assert.equal(parseMaxRss("Command exited with non-zero status 1\n98765\n"), 98765);
  assert.equal(parseMaxRss(""), null);
  assert.equal(parseMaxRss("time: cannot run\n"), null);
});

test("the median of an odd and an even count", () => {
  assert.equal(median([5, 1, 3]), 3);
  assert.equal(median([4, 2, 8, 6]), 5);
});

test("a figure within its tolerance is ok, a smaller one is noted, a bigger one fails", () => {
  const states = (now) => Object.fromEntries(compareCell("x/min", was, now).map((r) => [r.metric, r.state]));
  assert.deepEqual(states({ bytes: 100000, gzip: 30000, rssKb: 50000 }), { bytes: "ok", gzip: "ok", rssKb: "ok" });
  // 0.05% of 100000 is 50 bytes
  assert.equal(states({ bytes: 100050, gzip: 30000, rssKb: 50000 }).bytes, "ok");
  assert.equal(states({ bytes: 100051, gzip: 30000, rssKb: 50000 }).bytes, "regressed");
  assert.equal(states({ bytes: 99000, gzip: 30000, rssKb: 50000 }).bytes, "improved");
  // gzip may move 0.2%, memory 10%
  assert.equal(states({ bytes: 100000, gzip: 30060, rssKb: 50000 }).gzip, "ok");
  assert.equal(states({ bytes: 100000, gzip: 30061, rssKb: 50000 }).gzip, "regressed");
  assert.equal(states({ bytes: 100000, gzip: 30000, rssKb: 55000 }).rssKb, "ok");
  assert.equal(states({ bytes: 100000, gzip: 30000, rssKb: 55001 }).rssKb, "regressed");
  assert.equal(states({ bytes: 100000, gzip: 30000, rssKb: 40000 }).rssKb, "improved");
});

test("memory that was not measured is skipped, any other missing figure fails", () => {
  const rows = compareCell("x/min", was, { bytes: 100000, gzip: 30000 });
  assert.deepEqual(rows.map((r) => [r.metric, r.state]), [["bytes", "ok"], ["gzip", "ok"]]);
  const lost = compareCell("x/min", was, { bytes: 100000, rssKb: 50000 });
  assert.equal(lost.find((r) => r.metric === "gzip").state, "regressed");
  const added = compareCell("x/min-map", was, { ...was, map: 10 });
  assert.equal(added.find((r) => r.metric === "map").state, "regressed");
});

test("a cell the baseline lacks, or one that was not built, is a failure", () => {
  const baseline = { tolerance: DEFAULT_TOLERANCE, cells: { "a/min": was, "b/min": was } };
  const rows = compareAll(baseline, { "a/min": was, "c/min": was });
  const failed = rows.filter((r) => r.state === "regressed").map((r) => `${r.cell}: ${r.note}`);
  assert.deepEqual(failed.sort(), ["b/min: in the baseline but not built", "c/min: not in the baseline"]);
});

test("the tolerance in the baseline file overrides the default", () => {
  const baseline = { tolerance: { bytes: 0.01 }, cells: { "a/min": was } };
  const rows = compareAll(baseline, { "a/min": { ...was, bytes: 100900 } });
  assert.equal(rows.find((r) => r.metric === "bytes").state, "ok");
});

test("the table lists only what moved, with the change as a percentage", () => {
  const rows = compareCell("react/min", was, { bytes: 101000, gzip: 30000, rssKb: 50000 });
  const table = renderTable(rows);
  assert.match(table, /react\/min\s+bytes\s+100,000\s+101,000\s+\+1\.00%\s+REGRESSED/);
  assert.doesNotMatch(table, /gzip/);
  assert.equal(renderTable(compareCell("x", was, was)), "");
  assert.match(renderTable(compareCell("x", was, was), true), /gzip/);
});
