// Where the packages the tests load are installed (the libraries and compilers the differential and interop tests use; nothing here is built into rkp).
// RATHKIT_TEST_DEPS names another folder that has a node_modules, for example a copy inside a container.
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const testDeps = process.env.RATHKIT_TEST_DEPS ?? join(root, 'test-deps');
export const testModules = join(testDeps, 'node_modules');
export const installHint = 'pnpm install --dir test-deps --frozen-lockfile --ignore-scripts';
