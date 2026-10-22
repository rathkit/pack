module.exports = function (source) {
  const callback = this.async();
  const { tag } = this.getOptions();
  setTimeout(() => {
    const html = source.split('\n').filter(Boolean).map((line) => (line.startsWith('# ') ? `<h1>${line.slice(2)}</h1>` : `<p>${line}</p>`)).join('');
    callback(null, `export default ${JSON.stringify(`<${tag}>${html}</${tag}>`)};\nexport const resource = ${JSON.stringify(require('node:path').basename(this.resourcePath))};`);
  }, 5);
};
