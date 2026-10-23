// A runtime plugin: renames a request, answers for a remote that is gone, and notes what loaded.
export default function (options) {
  return {
    name: 'test-plugin',
    beforeRequest(args) {
      if (args.id === 'r2/Old') args.id = 'r2/utils';
      return args;
    },
    errorLoadRemote(args) {
      if (args.id === 'r2/Gone') return { __esModule: true, default: 'fallback for ' + args.id + ' ' + (options?.note ?? '') };
    },
    onLoad(args) {
      (globalThis.__loaded ??= []).push(args.id);
    },
  };
}
