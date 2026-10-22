// No import or export: a script, which the bundler wraps as a CommonJS module.
function tag(_v: any, context: any) {
  console.log('decorate', context.kind, String(context.name));
}

@tag
class Top {
  @tag run() { return 'top'; }
  accessor n = 4;
}

console.log(new Top().run(), new Top().n, Top.name);
