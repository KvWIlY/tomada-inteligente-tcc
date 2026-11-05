"""Sensibilidade ao morador simulado (revisao do orientador, 4.6).

O simulador apertava "Usar Agora" no mesmo passo em que o aparelho cortado voltava a ser usado:
obediencia perfeita, custo zero. Aqui o mesmo caso roda com um morador que demora para apertar
(`usuario_atraso_s`: o uso negado nesse intervalo e contado) e com um que desiste da tomada
(`usuario_desiste_apos`: a que pediu mais que N acionamentos num dia vira essencial no seguinte).
Uso: python usuario.py   (saida em resumo.txt; rastros apagados no fim)
"""
import csv
import json
import re
import shutil
import subprocess
import tomllib
from pathlib import Path

AQUI = Path(__file__).resolve().parent
SIM = AQUI.parents[1] / "simulador" / "target" / "release" / "simulador.exe"
AV = AQUI.parent / "avaliacao-busca-exata"
CASOS = ["c1-solteiro", "c2-familia", "c3-home-office", "c4-eletronicos"]
VARIANTES = [  # (rotulo, linhas a acrescentar em [casa])
    ("imediato", ""),
    ("atraso-30s", "usuario_atraso_s = 30\n"),
    ("atraso-2min", "usuario_atraso_s = 120\n"),
    ("desiste-3", "usuario_desiste_apos = 3\n"),
    ("desiste-1", "usuario_desiste_apos = 1\n"),
]


def caso_variante(caso, rotulo, extra):
    texto = (AV / f"{caso}.toml").read_text(encoding="utf-8")
    texto = re.sub(r'^nome = "[^"]+"', f'nome = "{caso}-{rotulo}"', texto, count=1, flags=re.M)
    texto = texto.replace("[casa]\n", "[casa]\n" + extra, 1)
    base = re.search(r'^arquivo = "([^"]+)"', texto, re.M).group(1)
    texto = texto.replace(f'arquivo = "{base}"', f'arquivo = "{(AV / base).as_posix()}"', 1)
    destino = AQUI / "casos" / f"{caso}-{rotulo}.toml"
    destino.parent.mkdir(exist_ok=True)
    destino.write_text(texto, encoding="utf-8")
    return destino


def medir(toml):
    with open(toml, "rb") as f:
        grupos = tomllib.load(f)["grupo"]
    lim = {g["nome"]: g.get("limiar_uso_w", 15) for g in grupos}
    subprocess.run([SIM, "rodar", "--realimentar", "--rastro", toml], cwd=AQUI, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    pasta = sorted((AQUI / "rodadas" / toml.stem).glob("2*"))[-1]
    resumo = json.loads((pasta / "resumo.json").read_text(encoding="utf-8"))
    with open(pasta / "tomadas.csv", encoding="utf-8") as f:
        tomadas = list(csv.DictReader(f))
    criticas = {g["nome"] for g in grupos if g.get("critica")}
    gerenciadas = [t["tomada"] for t in tomadas if t["tomada"] not in criticas and t["tomada"].rsplit("-", 1)[0] not in criticas]
    with open(pasta / "eventos.csv", encoding="utf-8") as f:
        desistiu = {e["tomada"]: int(e["passo"]) * 10 // 86400 for e in csv.DictReader(f) if e["tipo"] == "desistencia"}
    negado_wh = negado_min = 0.0
    with open(pasta / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            n, b = l["tomada"], float(l["base_w"])
            if l["estado"] == "OFF" and b > lim.get(n, lim.get(n.rsplit("-", 1)[0], 15)):
                negado_wh += b * 10 / 3600
                negado_min += 10 / 60
    shutil.rmtree(pasta)
    return dict(espera=(resumo["economia_wh"] - negado_wh) / 1000, negado=negado_wh / 1000, negado_min=negado_min,
                botoes=sum(int(t["botoes_usar_agora"]) for t in tomadas) / 30,
                desistiu=desistiu, gerenciadas=len(gerenciadas))


br = lambda x, d=2: f"{x:,.{d}f}".replace(",", "X").replace(".", ",").replace("X", ".")
linhas = ["caso | morador | espera cortada kWh | uso negado kWh (min) | Usar Agora/dia | tomadas que viraram essenciais (dia)"]
for caso in CASOS:
    for rotulo, extra in VARIANTES:
        r = medir(caso_variante(caso, rotulo, extra))
        d = ", ".join(f"{t} ({dia})" for t, dia in sorted(r["desistiu"].items(), key=lambda x: x[1])) or "-"
        linhas.append(f"{caso} | {rotulo} | {br(r['espera'], 3)} | {br(r['negado'], 3)} ({br(r['negado_min'], 0)}) | "
                      f"{br(r['botoes'], 1)} | {len(r['desistiu'])} de {r['gerenciadas']}: {d}")
        print(linhas[-1], flush=True)
(AQUI / "resumo.txt").write_text("\n".join(linhas) + "\n", encoding="utf-8")
