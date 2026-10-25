declare module "*.svg" {
  const source: string;
  export default source;
}

declare module "*.png" {
  const data_url: string;
  export default data_url;
}
