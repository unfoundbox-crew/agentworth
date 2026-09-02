// The Archie poses are imported as source, not through an <img>: a document's CSS
// custom properties do not reach inside an image, and the colourways are custom
// properties. Vite's `?raw` suffix does the work; this is what makes `tsc` agree.
// See packages/ui/brand/archie/README.md.
declare module "*.svg?raw" {
  const source: string;
  export default source;
}
