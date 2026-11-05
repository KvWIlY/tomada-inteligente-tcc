"""Tira dos logs e rastros os numeros das tabelas do capitulo de avaliacao. Uso: python tabelas.py"""
import csv, re, tomllib
from pathlib import Path

CEN = Path(__file__).resolve().parent


def log(caso):
    t = (CEN / f"log-{caso}.txt").read_text(encoding="utf-8")
    num = lambda p: [float(x) for x in re.search(p, t).groups()]
    tb, tp, et = num(r"tomadas: base ([\d.]+) kWh, com PL ([\d.]+) kWh, economia ([\d.]+)%")
    cb, cp, ec = num(r"casa: normal ([\d.]+) kWh, com PL ([\d.]+) kWh, economia ([\d.]+)%")
    kwh, rs = num(r"economia: ([\d.]+) kWh = R\$ ([\d.]+)")
    qs, qc = num(r"sem sistema (\d+), com sistema (\d+)")
    cortes = int(re.search(r"cortes (\d+)", t).group(1))
    return dict(tb=tb, tp=tp, et=et, cb=cb, cp=cp, ec=ec, kwh=kwh, rs=rs, qs=int(qs), qc=int(qc), cortes=cortes)


def negado(caso):
    """Minutos e kWh de uso negado por aparelho: OFF com a base acima do limiar de uso."""
    with open(CEN / f"{caso}.toml", "rb") as f:
        lim = {g["nome"]: g.get("limiar_uso_w", 15) for g in tomllib.load(f)["grupo"]}
    pasta = sorted((CEN / "rodadas" / caso).glob("2*"))[-1]
    minutos, wh = {}, 0.0
    with open(pasta / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            if l["estado"] != "OFF":
                continue
            n = l["tomada"]
            g = n if n in lim else n.rsplit("-", 1)[0]
            b = float(l["base_w"])
            if b > lim.get(g, 15):
                minutos[n] = minutos.get(n, 0) + 10 / 60
                wh += b * 10 / 3600
    return {k: round(v) for k, v in minutos.items()}, wh / 1000


br = lambda x, d=2: f"{x:,.{d}f}".replace(",", "X").replace(".", ",").replace("X", ".")
CASOS = ["c1-solteiro", "c2-familia", "c3-home-office", "c4-eletronicos", "c5a-cego", "c5b-reserva", "c5c-reserva-horario"]
print("caso | E_base casa | E_PL casa | eco tomadas | eco casa | R$ | kWh | acima sem/com | cortes | negado")
for c in CASOS:
    r = log(c)
    m, kn = negado(c)
    print(f"{c} | {br(r['cb'])} | {br(r['cp'])} | {br(r['et'])} | {br(r['ec'])} | {br(r['rs'])} | {br(r['kwh'],3)} | {r['qs']}/{r['qc']} | {r['cortes']} | {m} {br(kn)} kWh | tomadas base {br(r['tb'])} PL {br(r['tp'])}")
print()
for c in ["c4-w1", "c4-eletronicos", "c4-w100"]:
    r = log(c)
    print(f"{c}: E_PL tomadas {br(r['tp'])}, eco {br(r['et'])}%, R$ {br(r['rs'])}, cortes {r['cortes']}")
print()
for grupo in ("sorteio", "fds"):
    rs = [log(f"c4-{grupo}-s{s}") for s in (1, 2, 3)]
    faixa = lambda k, d=2: f"{br(min(r[k] for r in rs), d)} a {br(max(r[k] for r in rs), d)}"
    print(f"{grupo}: base tomadas {faixa('tb')}, eco {faixa('et')}%, kWh {faixa('kwh')}, R$ {faixa('rs')}, cortes {[r['cortes'] for r in rs]}, acima {[r['qc'] for r in rs]}, negado {[negado(f'c4-{grupo}-s{s}')[1] for s in (1,2,3)]}")
