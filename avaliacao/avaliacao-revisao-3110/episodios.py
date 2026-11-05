"""Maior episodio e passos por faixa de I/In, por rodada. Uso: python episodios.py caso..."""
import csv, sys, tomllib
from pathlib import Path
for nome in sys.argv[1:]:
    p_max = tomllib.load(open(f"{nome}.toml", "rb"))["casa"]["p_max_w"]
    pasta = sorted(Path("rodadas", nome).glob("2*"))[-1]
    soma = {}
    for l in csv.DictReader(open(pasta / "rastro.csv", encoding="utf-8")):
        if l["estado"] != "OFF":
            soma[int(l["passo"])] = soma.get(int(l["passo"]), 0) + float(l["base_w"])
    for lim in (1.0, 1.13, 1.45):
        total = seq = maior = 0
        ant = None
        for k in sorted(soma):
            if soma[k] > lim * p_max + 1e-6:
                total += 1
                seq = seq + 1 if ant == k - 1 else 1
                maior, ant = max(maior, seq), k
        print(nome, f">{lim} In: {total} passos, maior {maior * 10} s")
    print(nome, f"pico {max(soma.values()) / p_max:.2f} In")
