// Adds three lines at the top and says so in a map, so a stack trace must still name the original line.
const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const vlq = (n) => { let v = n < 0 ? (-n << 1) | 1 : n << 1, out = ''; do { let d = v & 31; v >>>= 5; if (v) d |= 32; out += B64[d]; } while (v); return out; };

export default {
  plugins: [{
    name: 'prefix',
    transform(code, id) {
      if (!id.endsWith('main.js')) return null;
      const header = ['// generated 1', '// generated 2', '// generated 3'];
      const lines = code.split('\n');
      // Every generated line maps to the same column 0 of the line three above it.
      const mappings = [';', ';', ';'].join('') + lines.map((_, i) => vlq(0) + vlq(i === 0 ? 0 : 0) + vlq(i === 0 ? 0 : 1) + vlq(0)).join(';');
      return { code: header.join('\n') + '\n' + code, map: { version: 3, sources: [id], sourcesContent: [code], names: [], mappings } };
    },
  }],
};
