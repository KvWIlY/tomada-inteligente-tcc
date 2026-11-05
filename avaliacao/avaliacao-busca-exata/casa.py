"""Soma as tomadas (com a decisao do MILP) com as cargas fora do sistema, passo a passo.

uso: python casa.py <base-tomadas.csv> <base-fora.csv> <rastro.csv> <disjuntor_w> [tarifa]
O estado do passo k vale para [t_k, t_k+10s): OFF => a tomada consome 0, senao consome a base.
"""
import csv, sys

tom_csv, fora_csv, rastro_csv, disj = sys.argv[1:5]
disj, tarifa, passo_h = float(disj), float(sys.argv[5]) if len(sys.argv) > 5 else 0.673, 10 / 3600

tom, fora, ras = (csv.reader(open(p, encoding="utf-8")) for p in (tom_csv, fora_csv, rastro_csv))
cols = next(tom)[1:]; next(fora); next(ras)
e_tb = e_tp = e_f = 0.0  # Wh: tomadas base, tomadas com PL, fora
queda_base = queda_pl = 0
pendente = next(ras, None)
for k, (lt, lf) in enumerate(zip(tom, fora)):
    base = dict(zip(cols, map(float, lt[1:])))
    f = sum(map(float, lf[1:]))
    off = set()
    while pendente is not None and int(pendente[0]) == k:
        if pendente[6] == "OFF":
            off.add(pendente[2])
        pendente = next(ras, None)
    tb = sum(base.values())
    tp = tb - sum(base[c] for c in off)
    e_tb += tb * passo_h; e_tp += tp * passo_h; e_f += f * passo_h
    queda_base += tb + f > disj + 1e-6
    queda_pl += tp + f > disj + 1e-6
assert pendente is None, "rastro com mais passos que a base"

kwh = lambda wh: wh / 1000
cb, cp = e_tb + e_f, e_tp + e_f
print(f"tomadas: base {kwh(e_tb):.2f} kWh, com PL {kwh(e_tp):.2f} kWh, economia {100*(e_tb-e_tp)/e_tb:.2f}%")
print(f"fora do sistema: {kwh(e_f):.2f} kWh")
print(f"casa: normal {kwh(cb):.2f} kWh, com PL {kwh(cp):.2f} kWh, economia {100*(cb-cp)/cb:.2f}%")
print(f"economia: {kwh(cb-cp):.3f} kWh = R$ {kwh(cb-cp)*tarifa:.2f} (tarifa {tarifa})")
print(f"passos acima do disjuntor ({disj:.0f} W): sem sistema {queda_base}, com sistema {queda_pl}")
