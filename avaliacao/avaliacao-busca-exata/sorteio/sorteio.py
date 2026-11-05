"""Rotinas sorteadas nos Cenarios 1-4, sementes 1-30 (achado 4.2 do orientador, 2026-10-27).

Para cada .toml desta pasta (gerados pelo ../gerar-cenarios.py): gera a base, roda com
--realimentar --rastro, guarda uma linha em resultados.csv e apaga a base e o rastro (a semente
no .toml reproduz os dois). Retoma de onde parou. No fim, ou com --resumir, imprime por grupo a
media, o desvio-padrao, o minimo, o maximo e o IC 95% (t de Student) de cada metrica.

Uso: python sorteio.py [prefixo]   (ex.: c1 roda so os c1-solteiro-s*)
     python sorteio.py --resumir
"""
import csv
import json
import statistics
import subprocess
import sys
import tomllib
from pathlib import Path

AQUI = Path(__file__).resolve().parent
SIM = AQUI / "../../../simulador/target/release/simulador.exe"
TARIFA = 0.965232  # R$/kWh, a mesma do rodar-todos.sh
CAMPOS = ["caso", "grupo", "semente", "base_kwh", "sistema_kwh", "eco_pct", "eco_kwh", "eco_rs",
          "usar_agora_dia", "negado_kwh", "cortes"]
T29 = 2.045  # t de Student, 95%, 29 graus de liberdade


def rodar(toml):
    with open(toml, "rb") as f:
        caso = tomllib.load(f)
    lim = {g["nome"]: g.get("limiar_uso_w", 15) for g in caso["grupo"]}
    subprocess.run([SIM, "gerar-base", "--forcar", toml.name], cwd=AQUI, check=True, capture_output=True)
    subprocess.run([SIM, "rodar", "--realimentar", "--rastro", toml.name], cwd=AQUI, check=True, capture_output=True)
    pasta = sorted((AQUI / "rodadas" / caso["nome"]).glob("2*"))[-1]
    r = json.loads((pasta / "resumo.json").read_text(encoding="utf-8"))
    with open(pasta / "tomadas.csv", encoding="utf-8") as f:
        botoes = sum(int(t["botoes_usar_agora"]) for t in csv.DictReader(f))
    negado = 0.0  # uso negado: OFF com a base acima do limiar de uso (como o tabelas.py)
    with open(pasta / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            if l["estado"] == "OFF":
                n = l["tomada"]
                b = float(l["base_w"])
                if b > lim.get(n, lim.get(n.rsplit("-", 1)[0], 15)):
                    negado += b * 10 / 3600
    for arq in ("rastro.csv", "passos.csv", "eventos.csv"):
        (pasta / arq).unlink(missing_ok=True)
    (AQUI / caso["base"]["arquivo"]).unlink(missing_ok=True)
    base, sist = r["energia_base_wh"] / 1000, r["energia_com_sistema_wh"] / 1000
    nome = caso["nome"]
    return dict(caso=nome, grupo=nome.rsplit("-s", 1)[0], semente=caso["base"]["semente"],
                base_kwh=round(base, 3), sistema_kwh=round(sist, 3), eco_pct=round(100 * (base - sist) / base, 3),
                eco_kwh=round(base - sist, 3), eco_rs=round((base - sist) * TARIFA, 2),
                usar_agora_dia=round(botoes / 30, 2), negado_kwh=round(negado / 1000, 3), cortes=r["cortes"])


def resumir():
    with open(AQUI / "resultados.csv", encoding="utf-8") as f:
        linhas = list(csv.DictReader(f))
    grupos = {}
    for l in linhas:
        grupos.setdefault(l["grupo"], []).append(l)
    for g, ls in grupos.items():
        print(f"{g} ({len(ls)} sementes: {min(int(l['semente']) for l in ls)}-{max(int(l['semente']) for l in ls)})")
        for k in CAMPOS[3:]:
            v = [float(l[k]) for l in ls]
            m = statistics.mean(v)
            dp = statistics.stdev(v) if len(v) > 1 else 0.0
            ic = T29 * dp / len(v) ** 0.5 if len(v) == 30 else float("nan")
            print(f"  {k:15} media {m:9.3f}  dp {dp:7.3f}  min {min(v):9.3f}  max {max(v):9.3f}  IC95 +-{ic:.3f}")


if __name__ == "__main__":
    if sys.argv[1:] == ["--resumir"]:
        resumir()
        sys.exit()
    saida = AQUI / "resultados.csv"
    feitos = set()
    if saida.exists():
        with open(saida, encoding="utf-8") as f:
            feitos = {l["caso"] for l in csv.DictReader(f)}
    prefixo = sys.argv[1] if len(sys.argv) > 1 else ""
    novo = not saida.exists()
    with open(saida, "a", encoding="utf-8", newline="") as f:
        w = csv.DictWriter(f, fieldnames=CAMPOS)
        if novo:
            w.writeheader()
        for toml in sorted(AQUI.glob(f"{prefixo}*.toml")):
            if toml.stem in feitos:
                continue
            linha = rodar(toml)
            w.writerow(linha)
            f.flush()
            print(linha["caso"], linha["eco_pct"], linha["usar_agora_dia"], flush=True)
    resumir()
