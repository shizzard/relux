// Vite virtual module suffixes. `?inline` returns the asset's contents as a
// string at build time (used in `main.ts` to inline CSS into the bundle so
// the single-file viewer ships without external stylesheets).
declare module '*.css?inline' {
  const css: string;
  export default css;
}
