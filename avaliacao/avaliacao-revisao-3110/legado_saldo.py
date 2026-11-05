"""Espera cortada, uso negado, Usar Agora e saldo por rodada; faixas de consumo da Secao consumo-tomada."""
import csv, re, sys, tomllib
from pathlib import Path
FAIXA = {"sempre": ((0.85, 1.02), (1.37, 1.54)), "sleep": ((0.36, 0.61), (0.88, 1.13))}
print("caso\tespera_kwh\tnegado_kwh\tusar_agora_dia\tsaldo_sempre\tsaldo_sleep")
for nome in sys.argv[1:]:
    grupos = tomllib.load(open(f"{nome}.toml", "rb"))["grupo"]
    lim = {g["nome"]: g.get("limiar_uso_w", 15) for g in grupos}
    pasta = sorted(Path("rodadas", nome).glob("2*"))[-1]
    corte = neg = 0.0
    fechado, aberto = 0, 0
    for l in csv.DictReader(open(pasta / "rastro.csv", encoding="utf-8")):
        n = l["tomada"]
        g = n if n in lim else n.rsplit("-", 1)[0]
        b = float(l["base_w"])
        if l["estado"] == "OFF":
            aberto += 1
            if b > lim.get(g, 15): neg += b * 10 / 3.6e6
            else: corte += b * 10 / 3.6e6
        else:
            fechado += 1
    ev = sum(1 for l in csv.DictReader(open(pasta / "eventos.csv", encoding="utf-8")) if "usar" in l["tipo"].lower())
    def saldo(m):
        (a0, a1), (f0, f1) = FAIXA[m]
        kwh = lambda a, f: (aberto * a + fechado * f) * 10 / 3.6e6
        return f"{corte - kwh(a1, f1):+.2f} a {corte - kwh(a0, f0):+.2f}"
    print(f"{nome}\t{corte:.2f}\t{neg:.2f}\t{ev / 30:.1f}\t{saldo('sempre')}\t{saldo('sleep')}")
