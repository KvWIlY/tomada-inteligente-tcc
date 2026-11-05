"""Media e IC de 95% (t de Student, 30 sementes) das rodadas de perda, a partir de resultados.tsv."""
import csv, re, statistics as st
from pathlib import Path
T = 2.0452  # t(29; 0,975)
L = list(csv.DictReader(open(Path(__file__).parent / "resultados.tsv", encoding="utf-8"), delimiter="\t"))
for nome in ["c7-c10", "c7-c30", "c7-c50", "c7-todos10", "c7-todos30", "c7-todos50", "c6-todos10", "c6-todos30"]:
    r = [l for l in L if re.fullmatch(nome + r"-s\d+", l["caso"])]
    assert len(r) == 30
    for col in ["acima", "maior_seq", "corte_a_mais", "off_critica"]:
        v = [float(l[col]) for l in r]
        print(f"{nome:11s} {col:13s} {st.mean(v):6.2f} +- {T * st.stdev(v) / 30 ** .5:4.2f}  min {min(v):g} max {max(v):g}")
