module.exports = function (content) {
  return `export default ${JSON.stringify(Array.from(content))};`;
};
module.exports.raw = true;
