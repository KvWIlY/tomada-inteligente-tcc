"""Tabela das rodadas REFIT (rodar antes: sh rodar.sh). Uso: python resumo.py

Por casa e decisao: economia, cortes, "Usar Agora" por dia e o corte que parece uso abaixo do
limiar. A espera tipica de um aparelho e a mediana das leituras dele ate o limiar (15 W); um
passo OFF com a base acima de max(2 W, 2 x espera tipica) conta como uso cortado sem o morador
perceber: o simulador so aperta "Usar Agora" quando a base passa do limiar.
"""
import csv
import glob
import statistics
import tomllib

LIMIAR = 15.0
for casa in (1, 2, 3, 4, 5, 11):
    base = {}
    with open(f"refit-casa{casa}.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            for k, v in l.items():
                if k != "t_s":
                    base.setdefault(k, []).append(float(v))
    for decisao in ("milp", "regra", "prioridade", "centralizado"):
        with open(f"refit-casa{casa}-{decisao}.toml", "rb") as f:
            criticas = {g["nome"] for g in tomllib.load(f)["grupo"] if g.get("critica")}
        pasta = sorted(glob.glob(f"rodadas/refit-casa{casa}-{decisao}/*/"))[-1]
        with open(pasta + "tomadas.csv", encoding="utf-8") as f:
            tomadas = [t for t in csv.DictReader(f) if t["tomada"] not in criticas]
        espera = {t["tomada"]: statistics.median([v for v in base[t["tomada"]] if v <= LIMIAR] or [0])
                  for t in tomadas}
        uso_wh = {t["tomada"]: 0.0 for t in tomadas}
        with open(pasta + "rastro.csv", encoding="utf-8") as f:
            for l in csv.DictReader(f):
                n = l["tomada"]
                if n in uso_wh and l["estado"] == "OFF" and float(l["base_w"]) > max(2.0, 2 * espera[n]):
                    uso_wh[n] += float(l["base_w"]) * 10 / 3600
        eco = sum(float(t["economia_wh"]) for t in tomadas)
        uso = sum(uso_wh.values())
        botoes = sum(int(t["botoes_usar_agora"]) for t in tomadas)
        cortes = sum(int(t["cortes"]) for t in tomadas)
        detalhe = ", ".join(f"{t['tomada']} espera {espera[t['tomada']]:.0f} W eco {float(t['economia_wh']):.0f} Wh"
                            f" uso cortado {uso_wh[t['tomada']]:.0f} Wh" for t in tomadas)
        print(f"casa {casa:2} {decisao:5}: economia {eco / 1000:.2f} kWh/mes, dos quais uso abaixo do limiar "
              f"{uso / 1000:.2f}; espera de fato {(eco - uso) / 1000:.2f}; cortes {cortes}; "
              f"Usar Agora {botoes / 30:.1f}/dia | {detalhe}")
