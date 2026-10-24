// Writes the software bill of materials of a release: CycloneDX 1.5 JSON listing the runtime
// components of rathkit-pack. There are none, and this stops with an error when
// tools/check-zero-deps.mjs finds otherwise, so the file cannot say "zero" when it is not true.
//   node tools/make-sbom.mjs [OUT.json]       (default: sbom.cdx.json)
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const manifest = readFileSync(join(root, 'Cargo.toml'), 'utf8');
const lock = readFileSync(join(root, 'Cargo.lock'), 'utf8');
const field = (text, key) => new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, 'm').exec(text)?.[1];
const name = field(manifest, 'name');
const version = field(manifest, 'version');
const license = field(manifest, 'license');

// The same rules as the zero-dependency check (empty dependency tables, no build script, a lock file with only this package).
const check = spawnSync(process.execPath, [join(root, 'tools', 'check-zero-deps.mjs')], { encoding: 'utf8' });
if (check.status !== 0) {
  console.error(`sbom: not zero components:\n${check.stdout}${check.stderr}`);
  process.exit(1);
}

const sbom = {
  bomFormat: 'CycloneDX',
  specVersion: '1.5',
  version: 1,
  // The time is fixed by SOURCE_DATE_EPOCH when set, so the same commit gives the same file.
  metadata: {
    timestamp: new Date((process.env.SOURCE_DATE_EPOCH ? Number(process.env.SOURCE_DATE_EPOCH) : Date.now() / 1000) * 1000).toISOString(),
    tools: { components: [{ type: 'application', name: 'tools/make-sbom.mjs', version }] },
    component: { type: 'application', 'bom-ref': `${name}@${version}`, name, version, licenses: [{ license: { id: license } }], purl: `pkg:cargo/${name}@${version}` },
  },
  components: [],
  dependencies: [{ ref: `${name}@${version}`, dependsOn: [] }],
};
const out = process.argv[2] ?? 'sbom.cdx.json';
writeFileSync(out, JSON.stringify(sbom, null, 2) + '\n');
console.log(`sbom: ${name} ${version}, 0 components -> ${out}`);
