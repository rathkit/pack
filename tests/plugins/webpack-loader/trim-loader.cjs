// Runs first: normal loaders go from the last to the first.
module.exports = function (source) {
  return source.replace(/[ \t]+$/gm, '').replace(/^﻿/, '');
};
