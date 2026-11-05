# Experimentos do artigo

Scripts, casos e resultados da avaliação do TCC. Os nomes das pastas são os usados nos scripts:
eles encontram o simulador em `../simulador` e as outras pastas por caminho relativo.

## O que não está aqui

- **Bases de consumo (`.csv`) e pastas `rodadas/`:** somam dezenas de GB. As bases sintéticas são
  refeitas por `simulador gerar-base --forcar <caso>.toml` e as rodadas, por
  `simulador rodar --realimentar --rastro <caso>.toml`. O `digest` de cada rodada permite comparar
  as decisões com as registradas nos logs.
- **Base REFIT:** vem do registro Zenodo 5063428 (CC BY 4.0); `avaliacao-refit/refit.py` explica como
  baixar o trecho e gera os casos.
- **Logs do ensaio com ventilador e monitor:** trazem dados da rede local e ficam fora do repositório.

## Pastas

| Pasta | Conteúdo |
|---|---|
| `avaliacao-busca-exata/` | Cenários 1 a 6, variantes, sensibilidade, sorteio de rotinas e logs |
| `avaliacao-centralizado/` | Comparação com a decisão centralizada |
| `avaliacao-usuario/` | Sensibilidade ao comportamento do morador |
| `avaliacao-perda/` | Perda de pacotes e tomadas ausentes; `gerar.py` cria os casos |
| `avaliacao-refit/` | Execução sobre seis residências da base REFIT |
| `avaliacao-revisao-3110/` | Tomadas surdas no Cenário 6 e espera de aparelhos antigos |
| `reverificacao-2026-10-29/` | `rodar.sh` repete os experimentos até aquela data e `figuras/graficos.py` gera os gráficos |
| `parecer-revisor-2026-10-22/06-espnow-3-placas/` | Teste de coordenação com três placas ESP32 e seriais registradas |
| `revisao-orientador-2026-10-26/` | `saldo-medido.py`, que calcula o saldo líquido |
| `revisao-orientador-2026-10-31/` | Log do tempo de decisão no ESP32 (`bancada-solver-ampla.txt`), gerado por `firmware/examples/bancada.rs` |

## Como repetir

```sh
cd simulador && cargo build --release
cd ../avaliacao/reverificacao-2026-10-29 && sh rodar.sh
```

Antes da etapa da REFIT, baixe os trechos e rode `python refit.py PASTA_DOS_TRECHOS` em
`avaliacao-refit/`. O `rodar.sh` completo não foi executado a partir desta pasta; os scripts são os
mesmos das rodadas originais, com os ajustes de caminho e de nome indicados no próprio código.

A rodada completa leva horas e ocupa dezenas de GB em `rodadas/`. Sem as rodadas dos experimentos
anteriores, a comparação de digests do `rodar.sh` só encontra o que foi rodado antes na mesma máquina;
os digests de referência estão nos logs.

Mudanças só de comentário em `firmware/src/solver/` alteram o `fonte_firmware_sha256`, mas não o
`digest`, que é o resumo das decisões.
