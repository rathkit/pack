import flexbug from 'postcss-flexbug';
// The function gets the context (`env` and `cwd`) and may return plugin instances in an array.
export default (ctx) => ({ plugins: [flexbug(), ctx.env === 'production' ? { postcssPlugin: 'mark', fake: (css) => `.production{x:1}\n${css}` } : null, ctx.cwd ? { postcssPlugin: 'cwd', fake: (css) => `${css}\n.has-cwd{x:1}` } : null] });
