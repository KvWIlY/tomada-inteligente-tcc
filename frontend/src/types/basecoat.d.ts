// =============================================
// Tipos para o JavaScript do Basecoat (FR-005b)
// =============================================
//
// O pacote `basecoat-css` NAO publica `.d.ts` — verificado abrindo o tarball:
// zero arquivos de declaracao, sem campo `types`/`typings`, e `main` apontando
// para um `.css`. Sem esta declaracao, tocar em qualquer componente interativo
// de um `.ts` falha com TS7016.
//
// Declaramos APENAS a superficie que este projeto usa. Uma fachada completa da
// API do Basecoat seria grande e desatualizaria em silencio; um `any` solto
// perderia a tipagem justamente onde a chamada e dinamica.
//
// RISCO ASSUMIDO E REGISTRADO: mudanca de assinatura no Basecoat nao gera erro
// de compilacao aqui — gera erro em runtime. Por isso o shim e pequeno,
// localizado, e a versao do pacote esta pinada em 1.0.2 (FR-023).
//
// O suporte a graficos do Basecoat esta em BETA e a documentacao avisa que a
// API JavaScript pode mudar antes do 1.0. FR-024 define o gatilho de saida:
// falhar FR-012 ou FR-013 manda voltar aos graficos SVG proprios.

import type { ChartConfiguration } from 'chart.js';

declare global {
  interface Window {
    basecoat?: {
      /**
       * Monta um grafico Chart.js num `<canvas>`, aplicando os defaults do
       * Basecoat (cores tematizadas via `--chart-1..5`, tooltip externo e
       * legenda opcional gerada).
       *
       * O wrapper consome `window.Chart` — que `main.ts` expoe — em vez de
       * importar Chart.js por conta propria.
       */
      chart(
        alvo: string | HTMLCanvasElement,
        config: ChartConfiguration & { legend?: boolean },
      ): void;
    };
  }
}

export {};
