# Experimentos do artigo

Scripts, casos e resultados da avaliação do TCC. Os nomes das pastas são os usados nos scripts:
eles encontram o simulador em `../simulador` e as outras pastas por caminho relativo.

## Bases de consumo

As bases estão nesta pasta, junto dos casos que as usam, e `bases.sha256` traz o hash de cada uma.

| Bases | Origem |
|---|---|
| `avaliacao-busca-exata/*.csv`, `avaliacao-perda/*.csv` e `avaliacao-revisao-3110/*.csv` | Sintéticas, geradas pelo simulador (`gerar-base --forcar <caso>.toml`) |
| `avaliacao-refit/refit-casa*.csv` | Derivadas de 30 dias da base REFIT |

A REFIT foi publicada por Murray, Stankovic e Stankovic (*Scientific Data* 4, 160122, 2017,
[doi:10.1038/sdata.2016.122](https://doi.org/10.1038/sdata.2016.122)), sob licença CC BY 4.0, e os
dados originais estão em <https://zenodo.org/records/5063428>. `avaliacao-refit/refit.py` explica como
baixar o trecho de cada casa e faz a conversão.

**Exceção:** as 150 bases da análise de rotinas (5 grupos de cenários, 30 sementes) ocupam cerca de
1,5 GB e não estão aqui. `avaliacao-busca-exata/sorteio/sorteio.py` gera cada uma pela sua semente,
roda o caso e apaga a base; `sorteio/resultados.csv` guarda o resultado das 150 execuções.

## O que não está aqui

- **Pastas `rodadas/`:** somam dezenas de GB e são refeitas por
  `simulador rodar --realimentar --rastro <caso>.toml`. O `digest` de cada rodada permite comparar as
  decisões com as registradas nos logs.
- **Logs do ensaio com ventilador e monitor:** trazem dados da rede local.

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

O `rodar.sh` completo não foi executado a partir desta pasta; os scripts são os
mesmos das rodadas originais, com os ajustes de caminho e de nome indicados no próprio código.

A rodada completa leva horas e ocupa dezenas de GB em `rodadas/`. Sem as rodadas dos experimentos
anteriores, a comparação de digests do `rodar.sh` só encontra o que foi rodado antes na mesma máquina;
os digests de referência estão nos logs.

Mudanças só de comentário em `firmware/src/solver/` alteram o `fonte_firmware_sha256`, mas não o
`digest`, que é o resumo das decisões.
