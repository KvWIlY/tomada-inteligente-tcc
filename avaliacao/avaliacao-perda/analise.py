"""Uma linha de resultado por rodada de perda (rastro.csv + eventos.csv). Uso: python analise.py caso

Colunas (tab): caso, passos acima do disjuntor (soma da base das tomadas nao OFF > p_max),
maior sequencia acima (passos), pior excesso (W), uso negado (min: passo OFF com a base acima do
limiar), corte a mais (passos com uma tomada em uso OFF que caberia: soma + a base dela <= p_max),
passos OFF de critica, trocas de estado das tomadas em uso fora do inicio/fim do uso, passos com
alguma vizinha ausente (carga fixa ou esquecida), maior carga de ausentes vista (W), digest.
"""
import csv
import re
import sys
import tomllib
from pathlib import Path

AQUI = Path(__file__).resolve().parent
nome = sys.argv[1]
with open(AQUI / f"{nome}.toml", "rb") as f:
    t = tomllib.load(f)
p_max = t["casa"]["p_max_w"]
lim = {g["nome"]: g.get("limiar_uso_w", 15) for g in t["grupo"]}
critica = {g["nome"] for g in t["grupo"] if g.get("critica")}
pasta = sorted((AQUI / "rodadas" / nome).glob("2*"))[-1]

passos = {}
with open(pasta / "rastro.csv", encoding="utf-8") as f:
    for l in csv.DictReader(f):
        passos.setdefault(int(l["passo"]), []).append((l["tomada"], float(l["base_w"]), l["estado"]))

acima = seq = maior_seq = 0
pior = negado = a_mais = off_critica = trocas = 0.0
antes = {}
for k in sorted(passos):
    linha = passos[k]
    soma = sum(b for _, b, e in linha if e != "OFF")
    if soma > p_max + 1e-6:
        acima += 1
        seq += 1
        pior = max(pior, soma - p_max)
    else:
        seq = 0
    maior_seq = max(maior_seq, seq)
    if any(e == "OFF" and b > lim[n] and soma + b <= p_max for n, b, e in linha):
        a_mais += 1
    for n, b, e in linha:
        negado += e == "OFF" and b > lim[n]
        off_critica += e == "OFF" and n in critica
        # Troca durante o uso: o aparelho estava e continua em uso, e o estado mudou.
        if n in antes and antes[n][0] > lim[n] and b > lim[n] and antes[n][1] != e:
            trocas += 1
        antes[n] = (b, e)

ausente, maior_l = 0, 0.0
with open(pasta / "eventos.csv", encoding="utf-8") as f:
    vistos = set()
    for l in csv.DictReader(f):
        if l["tipo"] == "vizinha_ausente":
            vistos.add(l["passo"])
            maior_l = max(maior_l, float(re.search(r"carga_ausentes_w=([\d.]+)", l["detalhes"]).group(1)))
    ausente = len(vistos)
digest = re.search(r"digest: (sha256:\w+)", (AQUI / "logs" / f"{nome}.txt").read_text(encoding="utf-8")).group(1)
print("\t".join(map(str, [nome, acima, maior_seq, round(pior), round(negado * 10 / 60), int(a_mais),
                          int(off_critica), int(trocas), ausente, round(maior_l), digest[:20]])))
