"""Episodios acima do disjuntor lidos pela curva tempo-corrente (revisao do orientador, 4.7).

Para cada rodada, soma passo a passo as tomadas ligadas (base com o estado != OFF) e as cargas
fora do sistema, junta os passos consecutivos acima da corrente nominal em episodios e da a
duracao e a maior razao I/In = P/P_disjuntor de cada um. Faixas da NBR NM 60898 / IEC 60898-1
(In <= 63 A): ate 1,13 In nao atua em 1 h; 1,45 In atua em menos de 1 h; 2,55 In atua de 1 a
60 s (In <= 32 A) ou de 1 a 120 s (In > 32 A); disparo instantaneo a partir de 3-5 In (curva B)
ou 5-10 In (curva C). "sem sistema" e a base inteira ligada.
Uso: python disjuntor.py
"""
import csv
from pathlib import Path

CEN = Path(__file__).resolve().parent
# (rodada, base das tomadas, base fora do sistema, disjuntor W)
CASOS = [
    ("c5a-cego", "c5-limite.csv", "c5-limite-fora.csv", 5080),
    ("c5b-reserva", "c5-limite.csv", "c5-limite-fora.csv", 5080),
    ("c5c-reserva-horario", "c5-limite.csv", "c5-limite-fora.csv", 5080),
    ("c6-regra", "c6-cozinha.csv", None, 2540),
    ("c6-milp", "c6-cozinha.csv", None, 2540),
    ("c6-prioridade", "c6-cozinha.csv", None, 2540),
]


def soma_linhas(arquivo, colunas=None):
    with open(CEN / arquivo, encoding="utf-8") as f:
        r = csv.reader(f)
        cab = next(r)[1:]
        for l in r:
            yield sum(float(v) for c, v in zip(cab, l[1:]) if colunas is None or c in colunas)


def ligadas(rastro):
    """Potencia das tomadas nao-OFF, passo a passo."""
    passo, w = 0, 0.0
    with open(rastro, encoding="utf-8") as f:
        for l in csv.DictReader(f):
            if int(l["passo"]) != passo:
                yield w
                passo, w = int(l["passo"]), 0.0
            if l["estado"] != "OFF":
                w += float(l["base_w"])
    yield w


def episodios(potencias, disj):
    eps, dur, pico = [], 0, 0.0
    for p in list(potencias) + [0.0]:
        if p > disj + 1e-6:
            dur, pico = dur + 10, max(pico, p / disj)
        elif dur:
            eps.append((dur, pico))
            dur, pico = 0, 0.0
    return eps


def faixa(r):
    return "<=1,13" if r <= 1.13 else "1,13-1,45" if r <= 1.45 else "1,45-2,55" if r <= 2.55 else ">2,55"


def resumo(nome, eps):
    if not eps:
        return f"{nome}: nenhum episodio"
    por_faixa = {}
    for d, r in eps:
        f = por_faixa.setdefault(faixa(r), [0, 0, 0.0])
        f[0], f[1], f[2] = f[0] + 1, max(f[1], d), max(f[2], r)
    txt = "; ".join(f"{k} In: {n} episodios, o maior de {d} s, pico {r:.2f} In" for k, (n, d, r) in sorted(por_faixa.items()))
    return f"{nome}: {len(eps)} episodios | {txt}"


feito = set()
for caso, base, fora, disj in CASOS:
    fora_w = list(soma_linhas(fora)) if fora else None
    somar = lambda ps: [p + (fora_w[k] if fora_w else 0) for k, p in enumerate(ps)]
    if base not in feito:
        feito.add(base)
        print(resumo(f"{base} sem sistema", episodios(somar(soma_linhas(base)), disj)))
    rastro = sorted((CEN / "rodadas" / caso).glob("2*"))[-1] / "rastro.csv"
    print(resumo(caso, episodios(somar(ligadas(rastro)), disj)))
