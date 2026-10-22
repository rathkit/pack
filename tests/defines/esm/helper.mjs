export function helper() {
  return process.env.NODE_ENV === "production" ? "fast" : "checked";
}
