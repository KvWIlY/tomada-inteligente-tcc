# Ícones — Material Symbols

Set: **Material Symbols** (Google), licença **Apache 2.0** — texto integral em `LICENSE`, nesta
pasta. Navegados pelo [icones.js.org](https://icones.js.org/collection/material-symbols), que é o
navegador da coleção Iconify; os dados vieram do pacote `@iconify-json/material-symbols@1.2.89`,
que declara `"license": "Apache-2.0"` e `author: Google`.

Adotado em **2026-08-26**, no lugar do Lucide (ISC), a pedido do dono. Antes do Lucide houve uma
tentativa com Flaticon, descartada por não ser obtenível por automação — exige conta e devolve 403,
e os termos não puderam sequer ser lidos para verificar atribuição e direito de redistribuição.

A Apache 2.0 permite redistribuir, inclusive modificado, com duas obrigações que estão cumpridas
aqui: **incluir uma cópia da licença** (o arquivo `LICENSE` ao lado) e **preservar os avisos** (o
cabeçalho de comentário no topo de cada `.svg`, nomeando o ícone de origem e a licença).

## Mapa nome-no-código → nome-no-set

| Código | Material Symbols |
|---|---|
| `bolt` | `bolt` |
| `check` | `check` |
| `cloud` | `cloud` |
| `cpu` | `memory` |
| `edit` | `edit` |
| `gauge` | `speed` |
| `grid` | `grid-view` |
| `info` | `info` |
| `logout` | `logout` |
| `plug` | `power-plug` |
| `plus` | `add` |
| `swap` | `swap-horiz` |
| `warn` | `warning` |
| `sun` | `light-mode` |
| `moon` | `dark-mode` |

São **15**, e só os efetivamente usados são trazidos (FR-004d). Eram 13 até o seletor de tema
chegar com `sun`/`moon`. O `icone` da tomada é constante `'plug'` (dívida D-07), então não há
ícone por eletrodoméstico — não por falta de opção no set, que tem 16.302.

## A diferença que quebra código

**Material Symbols são formas preenchidas; Lucide eram traços.**

| | Lucide | Material Symbols |
|---|---|---|
| Pintura | `fill="none"` + `stroke="currentColor"` | `fill="currentColor"` |
| Espessura | `stroke-width` (o 3º argumento de `icon()`) | não existe |
| Terminações | `stroke-linecap` / `stroke-linejoin` | não se aplicam |

Por isso `icon()` perdeu o terceiro parâmetro na migração. Manter um argumento que não faz nada
seria convidar o próximo a acreditar nele.

## Por que os SVG são versionados e o pacote npm não é dependência

Nada em `src/` importa `@iconify-json/material-symbols` — nem em runtime, nem no build; os corpos
dos 15 ícones estão inlinados em `src/icons.ts`, porque as views montam marcação como string, de
forma síncrona, e não podem esperar uma requisição. Declarar o pacote violaria SC-008 (nenhuma
dependência declarada e não usada), e ele traz 16.302 ícones para usar 15.

Para trocar ou acrescentar um ícone: `npm i -D @iconify-json/material-symbols@<versão>`, copiar o
`body` do ícone em `icons.json`, e desinstalar.
