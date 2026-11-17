import { build } from 'esbuild';

await build({
  entryPoints: ['src/main.js'],
  bundle: true,
  outfile: 'dist/main.js',
  format: 'esm',
  platform: 'browser',
  minify: true,
  sourcemap: true,
  define: { __APP_VERSION__: '"1.2.3"', 'process.env.NODE_ENV': '"production"' },
  loader: { '.txt': 'text' },
  alias: { '@': './src' },
});
