export default { module: { rules: [
  { test: /\.png$/, use: [{ loader: 'url-loader', options: { limit: 10 } }] },
  { test: /\.svg$/, use: [{ loader: 'url-loader', options: { limit: 100000 } }] },
  { test: /\.woff2$/, use: [{ loader: 'file-loader', options: { name: '[name].[contenthash:8].[ext]' } }] },
] } };
