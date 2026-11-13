// `css.preprocessorOptions.scss.additionalData` is put before every Sass file, and `css.postcss` is a PostCSS configuration of its own.
export default {
  css: {
    preprocessorOptions: { scss: { additionalData: (code, file) => `$brand: rgb(1, 2, 3);\n${code}` } },
    postcss: { plugins: [{ postcssPlugin: 'inline', fake: (css) => `${css}\n.inline { x: 1; }` }] },
  },
};
