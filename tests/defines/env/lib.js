function mode() {
  if (process.env.NODE_ENV !== "production") {
    return "development build";
  }
  return "production build";
}

function feature() {
  return process.env.NODE_ENV === "production" ? require("./prod.cjs") : require("./dev.cjs");
}

module.exports = { mode, feature };
