#!/bin/sh
# Regera as bases e roda os 6 cenarios em sequencia (30 d, --realimentar --rastro),
# depois soma com as cargas fora do sistema (casa.py). Uso: sh rodar-todos.sh
set -e
cd "$(dirname "$0")"
SIM=../../simulador/target/release/simulador.exe
TARIFA=0.965232

for f in c1-solteiro c2-familia c3-home-office c4-eletronicos c5a-cego c5-limite-fora \
         c1-solteiro-fora c2-familia-fora c3-home-office-fora c4-eletronicos-fora; do
    $SIM gerar-base --forcar $f.toml
done

# caso | base das tomadas | base de fora | disjuntor (W)
while read caso tom fora disj; do
    echo "=== $caso"
    $SIM rodar --realimentar --rastro --progresso 30 $caso.toml > log-$caso.txt 2>&1
    pasta=$(ls -d rodadas/$caso/*/ | sort | tail -1)
    python casa.py $tom $fora ${pasta}rastro.csv $disj $TARIFA >> log-$caso.txt
    tail -5 log-$caso.txt
done <<EOF
c1-solteiro c1-solteiro.csv c1-solteiro-fora.csv 5080
c2-familia c2-familia.csv c2-familia-fora.csv 8890
c3-home-office c3-home-office.csv c3-home-office-fora.csv 6350
c4-eletronicos c4-eletronicos.csv c4-eletronicos-fora.csv 5080
c5a-cego c5-limite.csv c5-limite-fora.csv 5080
c5b-reserva c5-limite.csv c5-limite-fora.csv 5080
EOF
echo FIM
