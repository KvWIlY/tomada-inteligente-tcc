"""Gera os casos de perda de heartbeats (revisao 2610, 3.2: convergencia e corte em cascata).

Cada caso e o c7 (tres aquecedores identicos) ou o c6 (cozinha) da avaliacao-busca-exata, com
`[[casa.perda]]` no fim. Uso: python gerar.py  (depois sh rodar.sh)
"""
from pathlib import Path

AQUI = Path(__file__).resolve().parent
ORIGEM = AQUI.parent / "avaliacao-busca-exata"


def caso(origem, nome, perdas, semente=0, duracao=None):
    linhas = (ORIGEM / f"{origem}.toml").read_text(encoding="utf-8").splitlines()
    i = next(k for k, l in enumerate(linhas) if l.startswith("nome = "))
    linhas[i] = f'nome = "{nome}"'
    if duracao:
        i = next(k for k, l in enumerate(linhas) if l.startswith("duracao = "))
        linhas[i] = f'duracao = "{duracao}"'
    j = next(k for k, l in enumerate(linhas) if l.startswith("p_max_w"))
    linhas.insert(j + 1, f"perda_semente = {semente}")
    for p in perdas:
        linhas.append("[[casa.perda]]")
        linhas += [f"{k} = {v!r}".replace("'", '"') for k, v in p.items()]
    (AQUI / f"{nome}.toml").write_text("\n".join(linhas) + "\n", encoding="utf-8")
    return nome


casos = []
# Conferencia: perda nula tem que dar o mesmo digest do c7 sem perda.
casos.append(caso("c7-identicos", "c7-p0", [dict(prob=0.0)]))
# Surdez de 10 min no inicio do conflito das 18:00, todo dia: so uma tomada nao ouve as outras.
for t in ("a", "c"):
    casos.append(caso("c7-identicos", f"c7-surdo-{t}", [dict(recebe=f"aquecedor-{t}", janela="18:00-18:10")]))
# Perda aleatoria, o dia todo: so o aquecedor-c (assimetrica) ou cada enlace por si.
for p in (0.1, 0.3, 0.5):
    for s in range(1, 31):
        casos.append(caso("c7-identicos", f"c7-c{int(p * 100)}-s{s}", [dict(recebe="aquecedor-c", prob=p)], s))
        casos.append(caso("c7-identicos", f"c7-todos{int(p * 100)}-s{s}", [dict(prob=p)], s))
# Corte em cascata pela reserva L (c6, 30 d).
casos.append(caso("c6-milp", "c6-silencio-airfryer", [dict(envia="air-fryer", janela="19:05-19:25")]))
casos.append(caso("c6-milp", "c6-surda-geladeira", [dict(recebe="geladeira", janela="11:55-12:30")]))
for p in (0.1, 0.3):
    for s in range(1, 31):
        casos.append(caso("c6-milp", f"c6-todos{int(p * 100)}-s{s}", [dict(prob=p)], s, "7d"))
# Referencias sem perda, com o mesmo analise.py.
casos.append(caso("c7-identicos", "c7-sem-perda", []))
casos.append(caso("c6-milp", "c6-sem-perda", []))
(AQUI / "casos.txt").write_text("\n".join(casos) + "\n", encoding="utf-8", newline="\n")
print(len(casos), "casos")
