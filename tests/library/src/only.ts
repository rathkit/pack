// An entry whose only export is the default one.
import { shout } from './lib/text';
export default function only(): string {
  return shout('only');
}
