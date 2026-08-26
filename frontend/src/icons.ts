// =============================================
// Icones — Material Symbols (Google, Apache-2.0), inlinados
// =============================================
//
// Set: **Material Symbols** (https://fonts.google.com/icons), navegado pelo
// https://icones.js.org/collection/material-symbols. Os arquivos de origem e o
// texto INTEGRAL da Apache 2.0 vivem em `public/icons/`; o mapa
// nome-no-codigo -> nome-no-set esta em `public/icons/ORIGEM.md`.
//
// Estilo **outlined**, que e o padrao do proprio Google e o que se parece com o
// traco que a interface ja tinha. Onde a coleção nao oferece variante
// `-outline` — `check`, `logout`, `add`, `swap-horiz` — e porque o glifo nao tem
// interior a vazar: preenchido e contornado sao o mesmo desenho.
//
// Por que INLINE e nao `<img src="/icons/x.svg">`: as views montam marcacao como
// string, de forma sincrona, e nao podem esperar uma requisicao. Inlinar tambem
// deixa o icone herdar `currentColor`, que e o que permite um mesmo arquivo
// servir os dois temas e todos os estados de badge.
//
// **Material Symbols sao FORMAS, nao tracos**: o contorno do estilo outlined e
// desenhado como preenchimento. O Lucide, usado ate 2026-08-26, era
// `fill="none"` + `stroke="currentColor"` + `stroke-width`; aqui e
// `fill="currentColor"` e espessura de traco nao existe. Por isso o terceiro
// parametro de `icon()` — a espessura — foi REMOVIDO: manter um argumento que
// nao faz nada seria convidar o proximo a acreditar nele.
//
// Apenas os icones efetivamente usados sao trazidos (FR-004d). Sao **15**: os 13
// de antes, mais `sun`/`moon`, do seletor de tema.

const PATHS: Record<string, string> = {
  bolt: "<path fill=\"currentColor\" d=\"m10.55 18.2l5.175-6.2h-4l.725-5.675L7.825 13H11.3zM8 22l1-7H4l9-13h2l-1 8h6L10 22zm3.775-9.75\"/>", // material-symbols:bolt-outline
  check: "<path fill=\"currentColor\" d=\"m9.55 18l-5.7-5.7l1.425-1.425L9.55 15.15l9.175-9.175L20.15 7.4z\"/>", // material-symbols:check
  cloud: "<path fill=\"currentColor\" d=\"M6.5 20q-2.275 0-3.887-1.575T1 14.575q0-1.95 1.175-3.475T5.25 9.15q.625-2.3 2.5-3.725T12 4q2.925 0 4.963 2.038T19 11q1.725.2 2.863 1.488T23 15.5q0 1.875-1.312 3.188T18.5 20zm0-2h12q1.05 0 1.775-.725T21 15.5t-.725-1.775T18.5 13H17v-2q0-2.075-1.463-3.538T12 6T8.463 7.463T7 11h-.5q-1.45 0-2.475 1.025T3 14.5t1.025 2.475T6.5 18m5.5-6\"/>", // material-symbols:cloud-outline
  cpu: "<path fill=\"currentColor\" d=\"M9 15V9h6v6zm2-2h2v-2h-2zm-2 8v-2H7q-.825 0-1.412-.587T5 17v-2H3v-2h2v-2H3V9h2V7q0-.825.588-1.412T7 5h2V3h2v2h2V3h2v2h2q.825 0 1.413.588T19 7v2h2v2h-2v2h2v2h-2v2q0 .825-.587 1.413T17 19h-2v2h-2v-2h-2v2zm8-4V7H7v10zm-5-5\"/>", // material-symbols:memory-outline
  edit: "<path fill=\"currentColor\" d=\"M5 19h1.425L16.2 9.225L14.775 7.8L5 17.575zm-2 2v-4.25L16.2 3.575q.3-.275.663-.425t.762-.15t.775.15t.65.45L20.425 5q.3.275.438.65T21 6.4q0 .4-.137.763t-.438.662L7.25 21zM19 6.4L17.6 5zm-3.525 2.125l-.7-.725L16.2 9.225z\"/>", // material-symbols:edit-outline
  gauge: "<path fill=\"currentColor\" d=\"M12 16.088q.95-.013 1.4-.688L19 7l-8.4 5.6q-.675.45-.712 1.375t.562 1.525t1.55.588M12 4q1.475 0 2.838.413T17.4 5.65l-1.9 1.2q-.825-.425-1.712-.637T12 6Q8.675 6 6.337 8.338T4 14q0 1.05.288 2.075T5.1 18h13.8q.575-.95.838-1.975T20 13.9q0-.9-.213-1.75t-.637-1.65l1.2-1.9q.75 1.175 1.188 2.5T22 13.85t-.325 2.725t-1.025 2.475q-.275.45-.75.7t-1 .25H5.1q-.525 0-1-.25t-.75-.7q-.65-1.125-1-2.387T2 14q0-2.075.788-3.887t2.15-3.175t3.187-2.15T12 4m.175 7.825\"/>", // material-symbols:speed-outline
  grid: "<path fill=\"currentColor\" d=\"M3 11V3h8v8zm0 10v-8h8v8zm10-10V3h8v8zm0 10v-8h8v8zM5 9h4V5H5zm10 0h4V5h-4zm0 10h4v-4h-4zM5 19h4v-4H5zM9 9\"/>", // material-symbols:grid-view-outline
  info: "<path fill=\"currentColor\" d=\"M11 17h2v-6h-2zm1.713-8.287Q13 8.425 13 8t-.288-.712T12 7t-.712.288T11 8t.288.713T12 9t.713-.288M12 22q-2.075 0-3.9-.788t-3.175-2.137T2.788 15.9T2 12t.788-3.9t2.137-3.175T8.1 2.788T12 2t3.9.788t3.175 2.137T21.213 8.1T22 12t-.788 3.9t-2.137 3.175t-3.175 2.138T12 22m0-2q3.35 0 5.675-2.325T20 12t-2.325-5.675T12 4T6.325 6.325T4 12t2.325 5.675T12 20m0-8\"/>", // material-symbols:info-outline
  logout: "<path fill=\"currentColor\" d=\"M5 21q-.825 0-1.412-.587T3 19V5q0-.825.588-1.412T5 3h7v2H5v14h7v2zm11-4l-1.375-1.45l2.55-2.55H9v-2h8.175l-2.55-2.55L16 7l5 5z\"/>", // material-symbols:logout
  plug: "<path fill=\"currentColor\" d=\"M11.5 19h1v-1.85l3.5-3.5V9H8v4.65l3.5 3.5zm-2 2v-3L6 14.5V9q0-.825.588-1.412T8 7h1L8 8V3h2v4h4V3h2v5l-1-1h1q.825 0 1.413.588T18 9v5.5L14.5 18v3zm2.5-7\"/>", // material-symbols:power-plug-outline
  plus: "<path fill=\"currentColor\" d=\"M11 13H5v-2h6V5h2v6h6v2h-6v6h-2z\"/>", // material-symbols:add
  swap: "<path fill=\"currentColor\" d=\"m7 20l-5-5l5-5l1.4 1.425L5.825 14H13v2H5.825L8.4 18.575zm10-6l-1.4-1.425L18.175 10H11V8h7.175L15.6 5.425L17 4l5 5z\"/>", // material-symbols:swap-horiz
  warn: "<path fill=\"currentColor\" d=\"M1 21L12 2l11 19zm3.45-2h15.1L12 6zm8.263-1.287Q13 17.425 13 17t-.288-.712T12 16t-.712.288T11 17t.288.713T12 18t.713-.288M11 15h2v-5h-2zm1-2.5\"/>", // material-symbols:warning-outline
  sun: "<path fill=\"currentColor\" d=\"M14.125 14.125Q15 13.25 15 12t-.875-2.125T12 9t-2.125.875T9 12t.875 2.125T12 15t2.125-.875m-5.663 1.413Q7 14.075 7 12t1.463-3.537T12 7t3.538 1.463T17 12t-1.463 3.538T12 17t-3.537-1.463M5 13H1v-2h4zm18 0h-4v-2h4zM11 5V1h2v4zm0 18v-4h2v4zM6.4 7.75L3.875 5.325L5.3 3.85l2.4 2.5zm12.3 12.4l-2.425-2.525L17.6 16.25l2.525 2.425zM16.25 6.4l2.425-2.525L20.15 5.3l-2.5 2.4zM3.85 18.7l2.525-2.425L7.75 17.6l-2.425 2.525zM12 12\"/>", // material-symbols:light-mode-outline
  moon: "<path fill=\"currentColor\" d=\"M12 21q-3.75 0-6.375-2.625T3 12t2.625-6.375T12 3q.35 0 .688.025t.662.075q-1.025.725-1.638 1.888T11.1 7.5q0 2.25 1.575 3.825T16.5 12.9q1.375 0 2.525-.613T20.9 10.65q.05.325.075.662T21 12q0 3.75-2.625 6.375T12 21m0-2q2.2 0 3.95-1.213t2.55-3.162q-.5.125-1 .2t-1 .075q-3.075 0-5.238-2.163T9.1 7.5q0-.5.075-1t.2-1q-1.95.8-3.163 2.55T5 12q0 2.9 2.05 4.95T12 19m-.25-6.75\"/>", // material-symbols:dark-mode-outline
};

/** Devolve o `<svg>` completo de um icone do set. Fallback = plug. */
export function icon(name: string, size = 18): string {
  const body = PATHS[name] ?? PATHS.plug;
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" aria-hidden="true">${body}</svg>`;
}
