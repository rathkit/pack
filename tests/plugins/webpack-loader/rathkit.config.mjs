import path from 'node:path';
const here = path.dirname(new URL(import.meta.url).pathname);

const markdownish = path.join(here, 'md-loader.cjs');
export default {
  module: {
    rules: [
      { test: /\.md$/, use: [{ loader: markdownish, options: { tag: 'section' } }, path.join(here, 'trim-loader.cjs')] },
      { test: /\.bin$/, use: path.join(here, 'raw-loader.cjs') },
    ],
  },
  plugins: [{ definitions: { 'process.env': { MODE: '"webpack-mode"' }, FLAG: true }, apply() {} }],
};
