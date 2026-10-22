import path, { join, sep as separator } from 'node:path';
import * as os from 'node:os';
import { readFileSync } from 'node:fs';
console.log(path.join('a', 'b'), join('c', 'd'), separator, typeof os.platform(), typeof readFileSync);
