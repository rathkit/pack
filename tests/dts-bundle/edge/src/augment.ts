declare global {
  interface Window { __lib: number }
}
declare module 'ext-pkg' {
  interface Ext { added?: boolean }
}
export {};
