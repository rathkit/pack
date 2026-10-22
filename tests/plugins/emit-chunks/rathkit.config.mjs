let seen = null;
const workers = {
  name: 'workers',
  transform(code, id) {
    if (!code.includes('__WORKER__')) return null;
    const ref = this.emitFile({ type: 'chunk', id: './worker.js', fileName: 'worker.mjs', importer: id });
    this.addWatchFile(id);
    return code.replace('__WORKER__', `import.meta.ROLLUP_FILE_URL_${ref}`);
  },
  moduleParsed(info) {
    if (info.id.endsWith('main.js')) seen = { imports: info.importedIds.map((i) => i.split('/').pop()).sort(), importers: info.importers.length, entry: info.isEntry };
    if (info.id.endsWith('shared.js')) seen = { ...seen, sharedImporters: info.importers.map((i) => i.split('/').pop()).sort(), exports: info.exports };
  },
  generateBundle(options, bundle) {
    const chunks = Object.values(bundle).filter((c) => c.type === 'chunk');
    const worker = bundle['worker.mjs'];
    this.emitFile({
      type: 'asset',
      fileName: 'meta.json',
      source: JSON.stringify({
        seen,
        worker: { entry: worker.isEntry, dynamic: worker.isDynamicEntry, facade: worker.facadeModuleId?.split('/').pop(), exports: worker.exports, modules: worker.moduleIds.map((m) => m.split('/').pop()).sort() },
        chunks: chunks.map((c) => (c.fileName.startsWith('chunk-') ? 'shared' : c.fileName)).sort(),
      }),
    });
  },
};
export default { plugins: [workers] };
