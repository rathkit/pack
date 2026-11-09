// docs/ROADMAP.md and the issues must agree: an item that is ticked has a closed issue, and an item whose issue is closed is ticked.
//   node tools/check-roadmap.mjs [--warn-open] [--offline STATES.json] [--project-offline NUMBERS.json]
// The issue states come from the GitHub API (GITHUB_TOKEN or GH_TOKEN, when set, is used; the repository is GITHUB_REPOSITORY or
// the `origin` remote). `--warn-open` turns "ticked but the issue is still open" into a warning: in a pull request the issue is
// closed by the merge, after this check has run. `--offline` reads `{ "7": "closed", ... }` instead of asking GitHub (the test).
// Every roadmap issue must also be an item of the public project (organization project 1), which has no automatic add: an issue
// filed later and left off it does not show on the project view. `--project-offline` reads `[7, 8, ...]` (issue numbers on the project).
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** The only repositories asked about: a roadmap link to any other repository is an error, so file text never decides where a request goes. */
const REPOSITORIES = ['rathkit/pack'];
/** The project the roadmap items must be on: its owner (an organization) and number. */
const PROJECT = { owner: 'rathkit', number: 1 };

/** The items of the roadmap that have an issue: `- [x] [3.5](https://github.com/o/r/issues/7) Title`. */
export function items(text) {
  const found = [];
  for (const line of text.split('\n')) {
    const m = /^- \[( |x)\] \[([^\]]+)\]\(https:\/\/github\.com\/([^/]+\/[^/]+)\/issues\/(\d+)\)/.exec(line);
    if (m) found.push({ ticked: m[1] === 'x', id: m[2], repo: m[3], issue: Number(m[4]) });
  }
  return found;
}

/** Problems and warnings, given the state ("open" or "closed") of each issue number. */
export function compare(list, states, warnOpen = false, onProject = null) {
  const problems = [];
  const warnings = [];
  const seen = new Set();
  for (const item of list) {
    if (seen.has(item.issue)) problems.push(`${item.id}: issue #${item.issue} is listed twice`);
    seen.add(item.issue);
    if (onProject && !onProject.has(item.issue)) problems.push(`${item.id}: issue #${item.issue} is not on the project (https://github.com/orgs/${PROJECT.owner}/projects/${PROJECT.number})`);
    const state = states[item.issue];
    if (!state) problems.push(`${item.id}: issue #${item.issue} was not found`);
    else if (item.ticked && state === 'open') (warnOpen ? warnings : problems).push(`${item.id}: ticked, but issue #${item.issue} is open`);
    else if (!item.ticked && state === 'closed') problems.push(`${item.id}: not ticked, but issue #${item.issue} is closed`);
  }
  return { problems, warnings };
}

async function states(list) {
  const arg = process.argv.indexOf('--offline');
  if (arg > 0) return JSON.parse(readFileSync(process.argv[arg + 1], 'utf8'));
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  const headers = { accept: 'application/vnd.github+json', 'user-agent': 'rathkit-pack-check-roadmap', ...(token ? { authorization: `Bearer ${token}` } : {}) };
  const result = {};
  for (const wanted of new Set(list.map((i) => i.repo))) {
    const repo = REPOSITORIES.find((r) => r === wanted);
    if (!repo) throw new Error(`docs/ROADMAP.md links to ${wanted}, which this check does not ask about`);
    for (let page = 1; ; page++) {
      const r = await fetch(`https://api.github.com/repos/${repo}/issues?state=all&per_page=100&page=${page}`, { headers });
      if (!r.ok) throw new Error(`GitHub answered ${r.status} for ${repo} issues`);
      const batch = await r.json();
      for (const issue of batch) if (!issue.pull_request) result[issue.number] = issue.state;
      if (batch.length < 100) break;
    }
  }
  return result;
}

/** The numbers of the issues of `repo` that are items of the project, or null when the project is not asked about. */
async function projectIssues(repo) {
  const arg = process.argv.indexOf('--project-offline');
  if (arg > 0) return new Set(JSON.parse(readFileSync(process.argv[arg + 1], 'utf8')));
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  if (!token) throw new Error('reading the project needs GITHUB_TOKEN or GH_TOKEN (GraphQL does not answer without one)');
  const query = `query($owner:String!,$number:Int!,$after:String){organization(login:$owner){projectV2(number:$number){items(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{content{... on Issue{number repository{nameWithOwner}}}}}}}}`;
  const found = new Set();
  let after = null;
  for (;;) {
    const r = await fetch('https://api.github.com/graphql', {
      method: 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json', 'user-agent': 'rathkit-pack-check-roadmap' },
      body: JSON.stringify({ query, variables: { owner: PROJECT.owner, number: PROJECT.number, after } }),
    });
    if (!r.ok) throw new Error(`GitHub answered ${r.status} for the project`);
    const body = await r.json();
    const project = body.data?.organization?.projectV2;
    if (!project) throw new Error(`could not read project ${PROJECT.owner}/${PROJECT.number}: ${(body.errors || []).map((e) => e.message).join('; ') || 'no such project'}`);
    for (const n of project.items.nodes) if (n.content?.number && n.content.repository.nameWithOwner === repo) found.add(n.content.number);
    if (!project.items.pageInfo.hasNextPage) return found;
    after = project.items.pageInfo.endCursor;
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const list = items(readFileSync(join(root, 'docs', 'ROADMAP.md'), 'utf8'));
  if (!list.length) {
    console.error('check-roadmap: no item with an issue link was found in docs/ROADMAP.md');
    process.exit(1);
  }
  const onProject = await projectIssues(REPOSITORIES[0]);
  const { problems, warnings } = compare(list, await states(list), process.argv.includes('--warn-open'), onProject);
  for (const w of warnings) console.log(`warning: ${w}`);
  for (const p of problems) console.error(`roadmap: ${p}`);
  if (problems.length) process.exit(1);
  console.log(`roadmap: ${list.length} items agree with their issues and are on the project${warnings.length ? ` (${warnings.length} warnings)` : ''}`);
}
