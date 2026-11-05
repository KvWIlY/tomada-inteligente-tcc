"""Quem foi cortado em uso, e o disjuntor, dos rastro.csv (parecer de 2026-10-22, itens 21 e 22).

Um "uso" e uma sequencia de passos com a base acima do limiar de uso do aparelho. Afetado: algum
passo dele em OFF; negado: todos. Disjuntor: soma da base das tomadas nao OFF contra p_max_w
menos as reservas da janela (vale para casas sem carga fora, como o c6; o c5b reserva o
chuveiro o dia todo e e conferido pelo casa.py). Uso: python teto.py caso [caso ...]
"""
import csv, sys, tomllib
from pathlib import Path

CEN = Path(__file__).resolve().parent


def minutos(hhmm):
    h, m = hhmm.split(":")
    return int(h) * 60 + int(m)


for caso in sys.argv[1:]:
    with open(CEN / f"{caso}.toml", "rb") as f:
        t = tomllib.load(f)
    lim = {g["nome"]: g.get("limiar_uso_w", 15) for g in t["grupo"]}
    reservas = [(*map(minutos, r["janela"].split("-")), r["w"]) for r in t["casa"].get("reserva", [])]
    pasta = sorted((CEN / "rodadas" / caso).glob("2*"))[-1]
    usos, em_uso, passos = {}, {}, {}
    with open(pasta / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            n, b, off = l["tomada"], float(l["base_w"]), l["estado"] == "OFF"
            g = n if n in lim else n.rsplit("-", 1)[0]
            u = usos.setdefault(n, dict(usos=0, afetados=0, negados=0, min=0.0, kwh=0.0))
            if b > lim.get(g, 15):
                if n not in em_uso:
                    em_uso[n] = [0, 0]  # passos, passos OFF
                    u["usos"] += 1
                em_uso[n][0] += 1
                em_uso[n][1] += off
                if off:
                    u["min"] += 10 / 60
                    u["kwh"] += b * 10 / 3.6e6
            elif n in em_uso:
                p, o = em_uso.pop(n)
                u["afetados"] += o > 0
                u["negados"] += o == p
            k = int(l["passo"])
            passos[k] = passos.get(k, 0.0) + (0.0 if off else b)
    dia = lambda k: (k * 10 // 60) % 1440
    teto = lambda k: t["casa"]["p_max_w"] - sum(w for a, b, w in reservas if a <= dia(k) < b)
    acima = [v - teto(k) for k, v in passos.items() if v > teto(k) + 1e-6]
    print(f"{caso}: passos acima do disjuntor {len(acima)} (pior {max(acima, default=0):.0f} W)")
    for n, u in usos.items():
        if u["usos"]:
            print(f"  {n}: usos {u['usos']}, afetados {u['afetados']}, negados {u['negados']}, "
                  f"{u['min']:.0f} min e {u['kwh']:.2f} kWh negados")
