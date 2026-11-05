"""MILP embarcado x MILP centralizado x regra de prioridade (revisao do orientador, 4.3).

Embarcado: cada tomada decide vendo as vizinhas pelo anuncio do passo anterior (o firmware).
Centralizado: o mesmo MILP vendo todas as tomadas no passo atual, sem atraso nem visao
divergente; todas resolvem a mesma entrada, o que equivale a um unico solver para a casa.
Prioridade: a regra fixa mais o corte das em uso pelo menor W_i ate caber no teto.

Para cada caso roda o mesmo TOML nos tres modos (--realimentar --rastro) e tira do rastro a
espera cortada, o uso negado (OFF com a base acima do limiar de uso), os passos acima do teto e
do disjuntor (tomadas ligadas + cargas fora do sistema) e o "Usar Agora" por dia.
Uso: python comparar.py   (saida em resumo.txt; o rastro de cada rodada e apagado no fim)
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
RF = AQUI.parent / "avaliacao-refit"
# (toml de origem, base das cargas fora do sistema, disjuntor em W)
CASOS = [
    (AV / "c1-solteiro.toml", AV / "c1-solteiro-fora.csv", 5080),
    (AV / "c2-familia.toml", AV / "c2-familia-fora.csv", 8890),
    (AV / "c3-home-office.toml", AV / "c3-home-office-fora.csv", 6350),
    (AV / "c4-eletronicos.toml", AV / "c4-eletronicos-fora.csv", 5080),
    (AV / "c5a-cego.toml", AV / "c5-limite-fora.csv", 5080),
    (AV / "c5b-reserva.toml", AV / "c5-limite-fora.csv", 5080),
    (AV / "c5c-reserva-horario.toml", AV / "c5-limite-fora.csv", 5080),
    (AV / "c6-milp.toml", None, 2540),
] + [(RF / f"refit-casa{c}-milp.toml", None, None) for c in (1, 2, 3, 4, 5, 11)]
MODOS = ("milp", "centralizado", "prioridade")


def caso_no_modo(origem, modo):
    """Copia o TOML com outro nome, outra decisao e a base por caminho absoluto."""
    texto = origem.read_text(encoding="utf-8")
    nome = re.search(r'^nome = "([^"]+)"', texto, re.M).group(1).removesuffix("-milp")
    texto = re.sub(r'^nome = "[^"]+"', f'nome = "{nome}-{modo}"', texto, count=1, flags=re.M)
    texto = re.sub(r'^decisao = "[^"]+"\n', "", texto, flags=re.M)
    texto = texto.replace("[casa]\n", f'[casa]\ndecisao = "{modo}"\n', 1)
    base = re.search(r'^arquivo = "([^"]+)"', texto, re.M).group(1)
    texto = texto.replace(f'arquivo = "{base}"', f'arquivo = "{(origem.parent / base).as_posix()}"', 1)
    destino = AQUI / "casos" / f"{nome}-{modo}.toml"
    destino.parent.mkdir(exist_ok=True)
    destino.write_text(texto, encoding="utf-8")
    return destino, nome


def medir(toml, fora, disjuntor):
    with open(toml, "rb") as f:
        lim = {g["nome"]: g.get("limiar_uso_w", 15) for g in tomllib.load(f)["grupo"]}
    subprocess.run([SIM, "rodar", "--realimentar", "--rastro", toml], cwd=AQUI, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    pasta = sorted((AQUI / "rodadas" / toml.stem).glob("2*"))[-1]
    resumo = json.loads((pasta / "resumo.json").read_text(encoding="utf-8"))
    with open(pasta / "tomadas.csv", encoding="utf-8") as f:
        botoes = sum(int(t["botoes_usar_agora"]) for t in csv.DictReader(f))
    cargas_fora = None
    if fora:
        with open(fora, encoding="utf-8") as f:
            cargas_fora = [sum(map(float, l[1:])) for l in list(csv.reader(f))[1:]]
    negado_wh, acima_disj, passo, ligado = 0.0, 0, None, 0.0

    def fecha(k, w):
        return disjuntor is not None and w + (cargas_fora[k] if cargas_fora else 0) > disjuntor + 1e-6

    with open(pasta / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            k = int(l["passo"])
            if k != passo:
                if passo is not None:
                    acima_disj += fecha(passo, ligado)
                passo, ligado = k, 0.0
            b = float(l["base_w"])
            if l["estado"] == "OFF":
                n = l["tomada"]
                if b > lim.get(n, lim.get(n.rsplit("-", 1)[0], 15)):
                    negado_wh += b * 10 / 3600
            else:
                ligado += b
    acima_disj += fecha(passo, ligado)
    shutil.rmtree(pasta)
    return dict(espera=(resumo["economia_wh"] - negado_wh) / 1000, negado=negado_wh / 1000,
                teto=resumo["passos_acima_do_teto"], excesso=resumo["pior_excesso_w"],
                disj=acima_disj if disjuntor else None, botoes=botoes / 30, cortes=resumo["cortes"])


br = lambda x, d=2: "-" if x is None else f"{x:,.{d}f}".replace(",", "X").replace(".", ",").replace("X", ".")
linhas = ["caso | modo | espera cortada kWh | uso negado kWh | passos acima do teto (pior excesso W) | "
          "passos acima do disjuntor | Usar Agora/dia | cortes"]
for origem, fora, disj in CASOS:
    for modo in MODOS:
        toml, nome = caso_no_modo(origem, modo)
        r = medir(toml, fora, disj)
        linhas.append(f"{nome} | {modo} | {br(r['espera'], 3)} | {br(r['negado'], 3)} | {r['teto']} ({br(r['excesso'], 0)}) | "
                      f"{'-' if r['disj'] is None else r['disj']} | {br(r['botoes'], 1)} | {r['cortes']}")
        print(linhas[-1], flush=True)
(AQUI / "resumo.txt").write_text("\n".join(linhas) + "\n", encoding="utf-8")
