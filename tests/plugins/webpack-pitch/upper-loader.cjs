module.exports = function (source) {
  const { suffix } = this.getOptions();
  return `export default ${JSON.stringify(source.trim().toUpperCase() + suffix)};`;
};
