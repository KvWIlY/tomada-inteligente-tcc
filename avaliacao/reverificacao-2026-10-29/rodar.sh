#!/bin/sh
# Reverificacao de 2026-10-29: roda de novo todos os experimentos do artigo (v0.2b/v0.3) a partir
# dos mesmos .toml e compara cada rodada com a anterior pelo digest das decisoes.
# Uso: sh rodar.sh   (saida em ./saida; os resultados antigos ficam em ./antes)
cd "$(dirname "$0")"
AQUI=$(pwd)
OK=$(cd .. && pwd)
SIM="$OK/../simulador/target/release/simulador.exe"
AV="$OK/avaliacao-busca-exata"
TARIFA=0.965232
mkdir -p antes saida

ultimo_digest() {  # pasta de rodadas de um caso -> digest da rodada mais recente
    d=$(ls -d "$1"/*/ 2>/dev/null | sort | tail -1)
    [ -n "$d" ] && python -c "import json,sys; print(json.load(open(sys.argv[1]+'resumo.json',encoding='utf-8'))['digest'])" "$d" 2>/dev/null
}

echo "=== 0. simulador e guarda do que existia"
(cd "$OK/../simulador" && cargo build --release 2>&1 | tail -1)
cp "$AV"/log-*.txt antes/ 2>/dev/null
cp "$AV/sorteio/resultados.csv" antes/sorteio-resultados.csv
cp "$OK/avaliacao-centralizado/resumo.txt" antes/centralizado-resumo.txt
cp "$OK/avaliacao-usuario/resumo.txt" antes/usuario-resumo.txt
cp "$OK/avaliacao-perda/resultados.tsv" antes/perda-resultados.tsv
: > antes/digests.txt
for d in "$AV"/rodadas/* "$OK"/avaliacao-refit/rodadas/*; do
    echo "$(basename "$d") $(ultimo_digest "$d")" >> antes/digests.txt
done

echo "=== 1. bases regeradas e comparadas byte a byte"
cd "$AV"
sha256sum *.csv > "$AQUI/antes/bases.sha256"
for t in c1-solteiro c2-familia c3-home-office c4-eletronicos c1-solteiro-fora c2-familia-fora \
         c3-home-office-fora c4-eletronicos-fora c5a-cego c5-limite-fora c6-milp c7-identicos \
         c4-pausas-t10 c4-sorteio-s1 c4-sorteio-s2 c4-sorteio-s3 c4-fds-s1 c4-fds-s2 c4-fds-s3; do
    "$SIM" gerar-base --forcar $t.toml > /dev/null 2>&1 || echo "gerar-base falhou: $t"
done
sha256sum *.csv > "$AQUI/saida/bases.sha256"
diff "$AQUI/antes/bases.sha256" "$AQUI/saida/bases.sha256" > "$AQUI/saida/bases.diff" \
    && echo "bases identicas" || echo "BASES DIFERENTES (ver saida/bases.diff)"

echo "=== 2. cenarios e variantes (todos os .toml de avaliacao-busca-exata)"
while read caso tom fora disj; do  # os que somam as cargas fora do sistema (casa.py)
    "$SIM" rodar --realimentar --rastro $caso.toml > log-$caso.txt 2>&1
    pasta=$(ls -d rodadas/$caso/*/ | sort | tail -1)
    [ "$fora" != "-" ] && python casa.py $tom $fora ${pasta}rastro.csv $disj $TARIFA >> log-$caso.txt
done <<LISTA
c1-solteiro c1-solteiro.csv c1-solteiro-fora.csv 5080
c2-familia c2-familia.csv c2-familia-fora.csv 8890
c3-home-office c3-home-office.csv c3-home-office-fora.csv 6350
c4-eletronicos c4-eletronicos.csv c4-eletronicos-fora.csv 5080
c5a-cego c5-limite.csv c5-limite-fora.csv 5080
c5b-reserva c5-limite.csv c5-limite-fora.csv 5080
c5c-reserva-horario c5-limite.csv c5-limite-fora.csv 5080
c4-w1 c4-eletronicos.csv c4-eletronicos-fora.csv 5080
c4-w100 c4-eletronicos.csv c4-eletronicos-fora.csv 5080
c4-sorteio-s1 c4-sorteio-s1.csv c4-eletronicos-fora.csv 5080
c4-sorteio-s2 c4-sorteio-s2.csv c4-eletronicos-fora.csv 5080
c4-sorteio-s3 c4-sorteio-s3.csv c4-eletronicos-fora.csv 5080
c4-fds-s1 c4-fds-s1.csv c4-eletronicos-fora.csv 5080
c4-fds-s2 c4-fds-s2.csv c4-eletronicos-fora.csv 5080
c4-fds-s3 c4-fds-s3.csv c4-eletronicos-fora.csv 5080
c6-milp c6-cozinha.csv - 2540
c6-regra c6-cozinha.csv - 2540
c6-prioridade c6-cozinha.csv - 2540
c6-milp-w3000 c6-cozinha.csv - 2540
c7-identicos c7-identicos.csv - 2540
LISTA
for t in c1-t* c2-t* c3-t* c4-t* c4-pausas-* c1-lim15 c2-lim15 c4-lim15; do
    caso=${t%.toml}
    [ -f $caso.toml ] && "$SIM" rodar --realimentar --rastro $caso.toml > log-$caso.txt 2>&1
done
python tabelas.py > "$AQUI/saida/tabelas.txt" 2>&1
python disjuntor.py > "$AQUI/saida/disjuntor.txt" 2>&1
python teto.py > "$AQUI/saida/teto.txt" 2>&1

echo "=== 3. sorteio (150 rodadas)"
rm -f sorteio/resultados.csv
(cd sorteio && python sorteio.py > "$AQUI/saida/sorteio.txt" 2>&1)
cp sorteio/resultados.csv "$AQUI/saida/sorteio-resultados.csv"

echo "=== 4. morador, centralizado"
(cd "$OK/avaliacao-usuario" && python usuario.py > /dev/null 2>&1; cp resumo.txt "$AQUI/saida/usuario-resumo.txt")
(cd "$OK/avaliacao-centralizado" && python comparar.py > /dev/null 2>&1; cp resumo.txt "$AQUI/saida/centralizado-resumo.txt")

echo "=== 5. REFIT (6 casas x 4 modos + limiar de 5 W)"
cd "$OK/avaliacao-refit"
for c in 1 2 3 4 5 11; do
    for d in milp regra prioridade centralizado; do
        "$SIM" rodar --realimentar --rastro refit-casa$c-$d.toml > log-casa$c-$d.txt 2>&1
    done
done
"$SIM" rodar --realimentar --rastro refit-casa11-limiar5.toml > log-casa11-limiar5.txt 2>&1
python resumo.py > "$AQUI/saida/refit.txt" 2>&1

echo "=== 6. perda de pacotes (247 casos; rodada apagada depois de analisada)"
cd "$OK/avaliacao-perda"
python gerar.py
mkdir -p logs
printf "caso\tacima\tmaior_seq\tpior_w\tnegado_min\tcorte_a_mais\toff_critica\ttrocas_em_uso\tpassos_ausente\tmaior_l_w\tdigest\n" > resultados.tsv
while read c; do
    "$SIM" rodar --realimentar --rastro $c.toml > logs/$c.txt 2>&1
    python analise.py $c >> resultados.tsv
    rm -rf "$(ls -d rodadas/$c/*/ | sort | tail -1)"
done < casos.txt
cp resultados.tsv "$AQUI/saida/perda-resultados.tsv"

echo "=== 7. saldo pelas folhas de dados"
cd "$OK/revisao-orientador-2026-10-26"
{ python saldo-medido.py 160 517 500; python saldo-medido.py 245 602 525; } > "$AQUI/saida/saldo.txt" 2>&1

echo "=== 8. digests: antes x depois"
cd "$AQUI"
: > saida/digests.txt
for d in "$AV"/rodadas/* "$OK"/avaliacao-refit/rodadas/*; do
    echo "$(basename "$d") $(ultimo_digest "$d")" >> saida/digests.txt
done
python - <<'PY'
antes = dict(l.split(" ", 1) for l in open("antes/digests.txt", encoding="utf-8").read().splitlines() if " " in l)
depois = dict(l.split(" ", 1) for l in open("saida/digests.txt", encoding="utf-8").read().splitlines() if " " in l)
iguais = [c for c in depois if c in antes and antes[c] == depois[c] and depois[c]]
difs = [c for c in depois if c in antes and antes[c] and antes[c] != depois[c]]
novos = [c for c in depois if c not in antes or not antes[c]]
print(f"rodadas iguais a anterior: {len(iguais)}; diferentes: {difs}; sem anterior: {novos}")
import csv
pa = {r["caso"]: r["digest"] for r in csv.DictReader(open("antes/perda-resultados.tsv", encoding="utf-8"), delimiter="\t")}
pd = {r["caso"]: r["digest"] for r in csv.DictReader(open("saida/perda-resultados.tsv", encoding="utf-8"), delimiter="\t")}
print(f"perda: {sum(pa.get(c) == d for c, d in pd.items())} de {len(pd)} iguais; diferentes: {[c for c, d in pd.items() if pa.get(c) != d][:20]}")
for nome in ("sorteio-resultados.csv",):
    a = sorted(open("antes/" + nome, encoding="utf-8").read().splitlines())
    b = sorted(open("saida/" + nome, encoding="utf-8").read().splitlines())
    print(f"{nome}: {'identico' if a == b else 'DIFERENTE'} ({len(b)} linhas)")
PY
for f in usuario-resumo.txt centralizado-resumo.txt; do
    diff -q antes/$f saida/$f > /dev/null && echo "$f: identico" || echo "$f: DIFERENTE (o centralizado ganhou as casas 3 e 4 da REFIT)"
done
echo FIM
