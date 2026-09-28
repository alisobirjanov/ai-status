// Vite hands over a file's text for an import ending in `?raw`.
declare module "*?raw" {
  const text: string;
  export default text;
}
