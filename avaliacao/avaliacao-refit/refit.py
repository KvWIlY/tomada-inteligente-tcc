"""Converte 30 dias da base REFIT (casas reais do Reino Unido, 8 s por aparelho) em bases e casos
do simulador. Resposta ao achado 4.2 do orientador (bases sinteticas). 2026-10-27.

Fonte: REFIT Electrical Load Measurements (Cleaned), Zenodo 5063428, CC BY 4.0,
Murray, Stankovic e Stankovic, Scientific Data 4, 160122 (2017), doi 10.1038/sdata.2016.122.
Rotulos dos aparelhos: metadados do conversor REFIT do NILMTK (buildingN.yaml).

Baixar so o trecho (o arquivo inteiro tem 250-440 MB e cobre ~2 anos), por exemplo:
  curl -L -r INICIO-FIM -o h5.part https://zenodo.org/api/records/5063428/files/CLEAN_House5.csv/content
com INICIO = 45% do tamanho e FIM = INICIO + 45 MB (cerca de 2 meses). O arquivo inteiro tambem serve.

Uso: python refit.py PASTA_DOS_TRECHOS   (le hN.part ou CLEAN_HouseN.csv; escreve aqui)

Tomadas gerenciadas: so eletronicos (TV, computador, som) e as cargas essenciais (geladeiras,
roteador). Micro-ondas, chaleira, torradeira, lavadoras e secadoras ficam de fora: passam do rele
de 10 A a 127 V ou nao sao candidatos ao corte. O medidor da REFIT (IAM) mede so corrente e assume
240 V, em watts inteiros: e potencia aparente arredondada, e espera abaixo de ~1 W le 0.
"""
import datetime as dt
import os
import sys

PASSO = 10
DIAS = 30
# casa: (inicio da janela de 30 d com menos lacunas, moradores, [(nome, ApplianceN, critica)])
CASAS = {
    1: ("2014-09-09", 2, [("geladeira", 1, True), ("freezer-1", 2, True), ("freezer-2", 3, True),
                          ("computador", 7, False), ("tv", 8, False)]),
    2: ("2014-08-22", 4, [("geladeira", 1, True), ("tv", 4, False), ("som", 7, False)]),
    # 3 e 4 entraram em 2026-10-29: com elas, as seis casas do registro do Zenodo. Janela escolhida
    # pelo mesmo criterio no trecho de 45% (lacunas de 18,5 e 19,0 h, todas de ate 1,1 h).
    3: ("2014-08-02", 2, [("geladeira-freezer", 2, True), ("freezer", 3, True), ("tv", 7, False)]),
    4: ("2014-08-02", 2, [("geladeira", 1, True), ("freezer", 2, True), ("geladeira-freezer", 3, True),
                          ("tv", 7, False)]),
    5: ("2014-08-07", 4, [("geladeira", 1, True), ("computador", 5, False), ("tv", 6, False)]),
    11: ("2014-12-24", 1, [("geladeira", 1, True), ("geladeira-freezer", 2, True),
                           ("computador", 5, False), ("roteador", 8, True), ("som", 9, False)]),
}


def ler(arq):
    with open(arq, encoding="utf-8", errors="replace") as f:
        for linha in f:
            c = linha.rstrip("\n").split(",")
            if len(c) != 13:
                continue  # cabecalho ou linha cortada nas pontas do trecho
            try:
                yield dt.datetime.strptime(c[0], "%Y-%m-%d %H:%M:%S"), [int(float(x)) for x in c[3:12]]
            except ValueError:
                continue


def converter(arq, casa):
    inicio, moradores, tomadas = CASAS[casa]
    t0 = dt.datetime.fromisoformat(inicio)
    fim = t0 + dt.timedelta(days=DIAS)
    n = DIAS * 86400 // PASSO
    saida = [None] * n
    ultimo, lacuna_s, anterior = None, 0.0, None
    k = 0
    for t, v in ler(arq):
        if t >= fim:
            break
        if anterior is not None and t >= t0 and (t - anterior).total_seconds() > 60:
            lacuna_s += (t - anterior).total_seconds()
        anterior = t
        # retencao de ordem zero: cada passo de 10 s recebe a ultima leitura ate ele
        while k < n and t0 + dt.timedelta(seconds=k * PASSO) < t:
            saida[k] = ultimo
            k += 1
        ultimo = [v[a - 1] for _, a, _ in tomadas]
    while k < n:
        saida[k] = ultimo
        k += 1
    if saida[0] is None:
        sys.exit(f"casa {casa}: o trecho nao comeca antes de {inicio}")
    base = f"refit-casa{casa}.csv"
    with open(base, "w", encoding="utf-8", newline="\n") as f:
        f.write("t_s," + ",".join(nome for nome, _, _ in tomadas) + "\n")
        for i, v in enumerate(saida):
            f.write(f"{i * PASSO}," + ",".join(map(str, v)) + "\n")
    for decisao in ("milp", "regra"):
        linhas = [f"# REFIT casa {casa} ({moradores} moradores), {inicio} + {DIAS} d, aparelhos reais medidos na tomada.",
                  f"# Lacunas no registro: {lacuna_s / 3600:.1f} h (a ultima leitura foi mantida).",
                  f'nome = "refit-casa{casa}-{decisao}"', 'duracao = "30d"', f"passo_s = {PASSO}",
                  "[base]", f'arquivo = "{base}"', f"passo_s = {PASSO}",
                  "[casa]", "p_max_w = 13800"]  # 60 A x 230 V: teto folgado, como nos Cenarios 1-4
        if decisao != "milp":
            linhas.append(f'decisao = "{decisao}"')
        for nome, _, critica in tomadas:
            linhas += ["[[grupo]]", f'nome = "{nome}"']
            linhas += ["critica = true"] if critica else ["peso = 10"]
        with open(f"refit-casa{casa}-{decisao}.toml", "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(linhas) + "\n")
    print(f"casa {casa}: {base}, lacunas {lacuna_s / 3600:.1f} h")


if __name__ == "__main__":
    pasta = sys.argv[1]
    for casa in CASAS:
        for nome in (f"h{casa}.part", f"CLEAN_House{casa}.csv"):
            if os.path.exists(os.path.join(pasta, nome)):
                converter(os.path.join(pasta, nome), casa)
                break
        else:
            print(f"casa {casa}: sem arquivo em {pasta}")
