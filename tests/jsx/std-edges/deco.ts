(Symbol as any).metadata ??= Symbol("Symbol.metadata");

export function note(text: string) {
  return (_value: any, context: { kind: string; name: string | symbol; metadata: any }) => {
    (context.metadata.notes ??= []).push(`${context.kind}:${String(context.name)}:${text}`);
  };
}

export function mark<T extends abstract new (...args: any) => any>(value: T, context: ClassDecoratorContext) {
  console.log('mark', String(context.name).replace('default_1', 'default'), Object.keys(context.metadata ?? {}).join());
  return value;
}
