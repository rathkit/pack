let r; try { r = require("./locale"); } catch (e) { r = {}; } module.exports = typeof r === "string" ? "loaded" : "ignored";
