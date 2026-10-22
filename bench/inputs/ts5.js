import ts from 'ts5';
const sf = ts.createSourceFile('a.ts', 'const x: number = 1; export class A<T> { m(a: T) { return a } }', ts.ScriptTarget.Latest, true);
console.log(ts.version, sf.statements.length, ts.SyntaxKind[sf.statements[0].kind]);
