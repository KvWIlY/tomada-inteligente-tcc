# Ícones — Lucide

Set: [Lucide](https://lucide.dev), licença **ISC** (ver `LICENSE` nesta pasta).

Escolhido na feature 007 depois que o Flaticon se mostrou inviável de obter por
automação: exige conta e devolve HTTP 403, e os termos não puderam sequer ser
lidos para verificar atribuição e direito de redistribuição.

A ISC permite copiar e redistribuir livremente, com uma condição: o aviso de
copyright acompanha as cópias. Por isso o `LICENSE` está versionado aqui, ao
lado dos arquivos — é o cumprimento da obrigação, não formalidade.

É também o set que o shadcn/ui usa, então a paridade visual com a documentação
que motivou esta feature vem de graça.

## Mapa nome-no-código → nome-no-lucide

| Código | Lucide |
|---|---|
| `bolt` | `zap` |
| `check` | `check` |
| `cloud` | `cloud` |
| `cpu` | `cpu` |
| `edit` | `pencil` |
| `gauge` | `gauge` |
| `grid` | `layout-grid` |
| `info` | `info` |
| `logout` | `log-out` |
| `plug` | `plug` |
| `plus` | `plus` |
| `swap` | `arrow-left-right` |
| `warn` | `triangle-alert` |

Só os 13 efetivamente usados foram copiados (FR-004d). O `icons.ts` antigo
declarava 26; os 13 de eletrodoméstico nunca são chamados, porque `icone` é
constante `'plug'` (dívida D-07).
