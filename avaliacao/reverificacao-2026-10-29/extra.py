"""Numeros que as correcoes do texto precisam, tirados das rodadas novas. Uso: python extra.py"""
import csv, json, glob
from pathlib import Path

AV = Path(__file__).resolve().parent.parent / "avaliacao-busca-exata"


def pasta(caso):
    return Path(sorted(glob.glob(str(AV / "rodadas" / caso / "2*")))[-1])


def tomadas(caso):
    with open(pasta(caso) / "tomadas.csv", encoding="utf-8") as f:
        return list(csv.DictReader(f))


def eco_pct(caso):
    ts = tomadas(caso)
    base = sum(float(t["energia_base_wh"]) for t in ts)
    return 100 * sum(float(t["economia_wh"]) for t in ts) / base, sum(int(t["botoes_usar_agora"]) for t in ts) / 30


print("== t_limite: economia nas tomadas (%) e Usar Agora/dia")
for c, nome in (("c1", "c1-solteiro"), ("c2", "c2-familia"), ("c3", "c3-home-office"), ("c4", "c4-eletronicos")):
    linha = []
    for t in (2, 5, 10, 20, 30, 60):
        caso = nome if t == 10 else f"{c}-t{t}"
        e, b = eco_pct(caso)
        linha.append(f"t{t}: {e:.2f}% {b:.1f}/d")
    print(c, " | ".join(linha))
print("== c4 com pausas")
for t in (2, 5, 10, 20, 30, 60):
    ts = tomadas(f"c4-pausas-t{t}")
    print(f"t{t}: eco {sum(float(x['economia_wh']) for x in ts)/1000:.2f} kWh, Usar Agora {sum(int(x['botoes_usar_agora']) for x in ts)/30:.1f}/dia")

print("== Usar Agora por tomada (fracao do total)")
for nome in ("c1-solteiro", "c2-familia", "c3-home-office", "c4-eletronicos"):
    ts = tomadas(nome)
    tot = sum(int(t["botoes_usar_agora"]) for t in ts)
    por = sorted(((int(t["botoes_usar_agora"]), t["tomada"]) for t in ts if int(t["botoes_usar_agora"])), reverse=True)
    print(nome, tot / 30, [(n, f"{b/tot:.0%}", round(b / 30, 1)) for b, n in por])

print("== c6: passos acima do teto por hora do dia, e W_i = 3000")
for caso in ("c6-milp", "c6-prioridade", "c6-milp-w3000"):
    r = json.load(open(pasta(caso) / "resumo.json", encoding="utf-8"))
    p_max = 2540.0
    soma, horas = {}, {}
    with open(pasta(caso) / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            if l["estado"] != "OFF":
                soma[int(l["passo"])] = soma.get(int(l["passo"]), 0.0) + float(l["base_w"])
    acima = sorted(k for k, w in soma.items() if w > p_max + 1e-6)
    for k in acima:
        h = (k * 10 % 86400) // 60
        horas[f"{h // 60:02d}:{h % 60:02d}"] = horas.get(f"{h // 60:02d}:{h % 60:02d}", 0) + 1
    negado = {}
    with open(pasta(caso) / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            if l["estado"] == "OFF" and float(l["base_w"]) > 15:
                negado[l["tomada"]] = negado.get(l["tomada"], 0) + 10 / 60
    print(caso, "digest", r["digest"][:20], "acima", len(acima), "por minuto do dia:", horas,
          "negado min:", {k: round(v) for k, v in negado.items()})

print("== limiar de 15 W em todas as tomadas (decodificador 11 W em uso)")
for orig, var in (("c1-solteiro", "c1-lim15"), ("c2-familia", "c2-lim15"), ("c4-eletronicos", "c4-lim15")):
    eo, bo = eco_pct(orig)
    ev, bv = eco_pct(var)
    cortado = {}
    with open(pasta(var) / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            n = l["tomada"]
            if l["estado"] == "OFF" and ((n.startswith("decodificador") and float(l["base_w"]) >= 11) or (n.startswith("soundbar") and float(l["base_w"]) >= 30)):
                cortado[n] = cortado.get(n, 0) + 10 / 3600
    print(f"{orig}: eco {eo:.2f}% -> {ev:.2f}%; Usar Agora {bo:.1f} -> {bv:.1f}/dia; horas de uso cortado: { {k: round(v, 1) for k, v in cortado.items()} }")
