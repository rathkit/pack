// Like style-loader: the work is in pitch, which hands the file to another loader through an inline request.
module.exports = function () {
  throw new Error('the normal phase of this loader must not run');
};
module.exports.pitch = function () {
  const request = JSON.stringify('!!' + require.resolve('./upper-loader.cjs') + '?{"suffix":"+"}!' + this.utils.contextify(this.context, this.resourcePath));
  return `import text from ${request};\nexport default text + " (via pitch)";\nexport const id = typeof module.id;`;
};
