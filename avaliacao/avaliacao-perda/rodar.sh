#!/bin/sh
# Roda os casos de casos.txt (python gerar.py antes) e resume cada um em resultados.tsv.
# Cada rodada fica em rodadas/ (~13 MB; ~3,5 GB ao todo): apagar a mao depois, se quiser.
# Uso: sh rodar.sh
set -e
cd "$(dirname "$0")"
SIM=../../simulador/target/release/simulador.exe
mkdir -p logs
printf "caso\tacima\tmaior_seq\tpior_w\tnegado_min\tcorte_a_mais\toff_critica\ttrocas_em_uso\tpassos_ausente\tmaior_l_w\tdigest\n" > resultados.tsv
while read c; do
    $SIM rodar --realimentar --rastro $c.toml > logs/$c.txt 2>&1
    python analise.py $c >> resultados.tsv
done < casos.txt
echo FIM
