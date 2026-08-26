// Modulos JS do Basecoat, importados so pelo efeito colateral.
//
// Sao IIFEs sem `export` — nao ha superficie a tipar, so a existencia do modulo
// a declarar para o `tsc`. O que eles instalam em `window` (o registro e
// `basecoat.chart`) esta tipado em `basecoat.d.ts`.
//
// Este arquivo NAO pode ter `import`/`export`: declaracao de modulo ambiente so
// vale em arquivo de escopo global.

declare module 'basecoat-css/basecoat.min';
declare module 'basecoat-css/sidebar.min';
declare module 'basecoat-css/toast.min';
declare module 'basecoat-css/chart.min';
