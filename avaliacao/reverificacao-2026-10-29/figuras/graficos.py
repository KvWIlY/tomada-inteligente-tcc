"""Figuras da avaliação do artigo v0.2 (2026-10-27), das rodadas da busca exata em
ok/avaliacao-busca-exata e das seriais do teste de 22/10. Cópia do script de 23/10 com duas
correções da revisão do orientador (4.10): o eixo da fig-t-limite começa em zero, e a
fig-espera-dia-c4 marca os trechos em que um aparelho está em uso (o consumo dele sai da soma
da espera, e a curva "sem o sistema" cai sem que nada tenha sido economizado).
Grava JPG a 300 dpi nesta pasta. Uso: python graficos.py

Paleta: slots categóricos do guia de visualização, validados contra o branco (ferro azul, air
fryer laranja, micro-ondas verde-água, cafeteira amarelo; geladeira e cargas fora em cinza). O
teto e o disjuntor são linha tracejada em tinta, porque são limite, não série.
"""
import csv
import tomllib
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Patch
from matplotlib.ticker import FuncFormatter

AQUI = Path(__file__).resolve().parent
SIM = AQUI.parents[1] / "avaliacao-busca-exata"
ESPNOW = AQUI.parents[1] / "parecer-revisor-2026-10-22" / "06-espnow-3-placas"
PASSO, DIA = 10, 10  # dia exibido: longe da calibração do começo da rodada
TINTA, TINTA2, MUDO, GRADE, EIXO = "#0b0b0b", "#52514e", "#898781", "#e1e0d9", "#c3c2b7"
AZUL, LARANJA, AQUA, AMARELO, CINZA = "#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#c3c2b7"
COR = {"ferro": AZUL, "air-fryer": LARANJA, "microondas": AQUA, "cafeteira": AMARELO, "geladeira": CINZA}
NOME = {"ferro": "Ferro", "air-fryer": "Air fryer", "microondas": "Micro-ondas", "cafeteira": "Cafeteira", "geladeira": "Geladeira"}

plt.rcParams.update({
    "font.family": "DejaVu Sans", "font.size": 9, "axes.edgecolor": EIXO, "axes.linewidth": 0.8,
    "axes.labelcolor": TINTA2, "xtick.color": TINTA2, "ytick.color": TINTA2, "text.color": TINTA,
    "axes.spines.top": False, "axes.spines.right": False, "axes.grid": True, "grid.color": GRADE,
    "grid.linewidth": 0.6, "grid.linestyle": "-", "axes.axisbelow": True, "legend.frameon": False,
    "lines.linewidth": 2, "lines.solid_capstyle": "round",
})
br = lambda v, d=0: f"{v:,.{d}f}".replace(",", "X").replace(".", ",").replace("X", ".")
milhar = FuncFormatter(lambda v, _: br(v))
hora_fmt = FuncFormatter(lambda v, _: f"{int(v // 60):02d}:{int(v % 60):02d}")


def salvar(fig, nome):
    fig.savefig(AQUI / f"{nome}.jpg", dpi=300, bbox_inches="tight", facecolor="white", pil_kwargs={"quality": 92})
    plt.close(fig)
    print("gravada", nome)


def rodada(caso):
    return sorted((SIM / "rodadas" / caso).glob("2*"))[-1]


def passo_de(hh, mm, dia=DIA):
    return (dia * 86400 + hh * 3600 + mm * 60) // PASSO


def janela(caso, ini, fim):
    """{tomada: {passo: (base_w, estado)}} entre os passos ini e fim (o rastro vem ordenado por passo)."""
    out = {}
    with open(rodada(caso) / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            k = int(l["passo"])
            if k < ini:
                continue
            if k >= fim:
                break
            out.setdefault(l["tomada"], {})[k] = (float(l["base_w"]), l["estado"])
    return out


def grupos(caso):
    with open(SIM / f"{caso}.toml", "rb") as f:
        return {g["nome"]: g for g in tomllib.load(f)["grupo"]}


def grupo_de(tomada, gs):
    return tomada if tomada in gs else tomada.rsplit("-", 1)[0]


def teto(ax, w, texto, x):
    ax.axhline(w, color=TINTA, lw=1, ls=(0, (4, 3)), zorder=5)
    ax.text(x, w * 1.03, texto, color=TINTA2, fontsize=8, va="bottom", ha="left")


# 1. Consumo em espera num dia do cenário 4 ------------------------------------------------
def fig_espera_c4():
    gs = grupos("c4-eletronicos")
    ini, fim = passo_de(0, 0), passo_de(0, 0, DIA + 1)
    jan = janela("c4-eletronicos", ini, fim)
    sem, com = [0.0] * (fim - ini), [0.0] * (fim - ini)
    em_uso = [False] * (fim - ini)
    for t, serie in jan.items():
        g = gs[grupo_de(t, gs)]
        if g.get("critica"):
            continue
        lim = g.get("limiar_uso_w", 15)
        for k, (b, e) in serie.items():
            if b <= lim:
                sem[k - ini] += b
                com[k - ini] += b if e != "OFF" else 0.0
            else:
                em_uso[k - ini] = True
    x = [i * PASSO / 60 for i in range(fim - ini)]
    cortado_wh = sum(s - c for s, c in zip(sem, com)) * PASSO / 3600
    fig, ax = plt.subplots(figsize=(6.3, 2.6))
    ax.fill_between(x, 0.93, 1, where=em_uso, step="post", transform=ax.get_xaxis_transform(), color=MUDO, alpha=0.6,
                    lw=0, label="Algum aparelho em uso (o consumo dele fica fora da soma)")
    ax.fill_between(x, com, sem, step="post", color=AZUL, alpha=0.12, lw=0, label=f"Espera cortada ({br(cortado_wh)} Wh no dia)")
    ax.step(x, sem, where="post", color=MUDO, lw=1.6, label="Sem o sistema")
    ax.step(x, com, where="post", color=AZUL, lw=1.6, label="Com o sistema")
    ax.set_xlim(0, 1440); ax.set_xticks(range(0, 1441, 180)); ax.xaxis.set_major_formatter(hora_fmt)
    ax.set_ylim(0, max(sem) * 1.25); ax.set_ylabel("Potência em espera (W)"); ax.yaxis.set_major_formatter(milhar)
    ax.set_yticks(range(0, int(max(sem) * 1.25) + 1, 5))
    ax.legend(loc="upper center", ncol=2, bbox_to_anchor=(0.5, 1.32), fontsize=8)
    salvar(fig, "fig-espera-dia-c4")


# 2. Energia não consumida: espera cortada x uso negado ------------------------------------
CASOS_ECO = [("c1-solteiro", "1. Solteiro"), ("c2-familia", "2. Família"), ("c3-home-office", "3. Home Office"),
             ("c4-eletronicos", "4. Eletrônicos"), ("c5a-cego", "5a. Sem reserva"), ("c5b-reserva", "5b. Reserva fixa"),
             ("c5c-reserva-horario", "5c. Reserva por horário")]


def economia_e_negado(caso):
    with open(rodada(caso) / "tomadas.csv", encoding="utf-8") as f:
        eco = sum(float(t["economia_wh"]) for t in csv.DictReader(f)) / 1000
    gs, neg = grupos(caso), 0.0
    with open(rodada(caso) / "rastro.csv", encoding="utf-8") as f:
        for l in csv.DictReader(f):
            if l["estado"] == "OFF":
                b = float(l["base_w"])
                if b > gs[grupo_de(l["tomada"], gs)].get("limiar_uso_w", 15):
                    neg += b * PASSO / 3.6e6
    return eco, neg


def fig_energia():
    dados = [(rot, *economia_e_negado(c)) for c, rot in CASOS_ECO]
    fig, ax = plt.subplots(figsize=(6.3, 2.9))
    ys = list(range(len(dados)))[::-1]
    for y, (rot, eco, neg) in zip(ys, dados):
        esp = eco - neg
        ax.barh(y, esp, height=0.55, color=AZUL, edgecolor="white", linewidth=1.5)
        if neg > 0.01:
            ax.barh(y, neg, left=esp, height=0.55, color=LARANJA, edgecolor="white", linewidth=1.5)
            txt = f"{br(eco, 2)} kWh, dos quais {br(neg, 2)} de uso negado"
        else:
            txt = f"{br(eco, 2)} kWh"
        ax.text(eco + 0.6, y, txt, va="center", fontsize=8, color=TINTA2)
    ax.set_yticks(ys); ax.set_yticklabels([d[0] for d in dados]); ax.tick_params(axis="y", length=0)
    ax.set_xlim(0, max(d[1] for d in dados) * 1.55); ax.set_xlabel("Energia não consumida no mês (kWh)")
    ax.xaxis.set_major_formatter(milhar); ax.grid(axis="y", visible=False)
    ax.legend(handles=[Patch(color=AZUL, label="Consumo em espera cortado"), Patch(color=LARANJA, label="Uso negado")],
              loc="upper right", fontsize=8)
    salvar(fig, "fig-energia-nao-consumida")


# 3. Cenário 5 entre 18:45 e 19:30 ----------------------------------------------------------
def fig_cenario5():
    ini, fim = passo_de(18, 45), passo_de(19, 30)
    fora = []
    with open(SIM / "c5-limite-fora.csv", encoding="utf-8") as f:
        r = csv.reader(f); next(r)
        for k, linha in enumerate(r):
            if k >= fim:
                break
            if k >= ini:
                fora.append(sum(map(float, linha[1:])))
    x = [(ini + i) * PASSO / 60 - DIA * 1440 for i in range(fim - ini)]
    fig, axs = plt.subplots(1, 3, figsize=(6.3, 2.4), sharey=True)
    for ax, (caso, titulo) in zip(axs, [("c5a-cego", "5a. Sem reserva"), ("c5b-reserva", "5b. Reserva fixa"),
                                        ("c5c-reserva-horario", "5c. Reserva por horário")]):
        jan = janela(caso, ini, fim)
        tom = [sum(s[k][0] for s in jan.values() if s[k][1] != "OFF") for k in range(ini, fim)]
        tot = [a + b for a, b in zip(fora, tom)]
        ax.fill_between(x, 0, fora, step="post", color=CINZA, alpha=0.8, lw=0, label="Cargas fora do sistema")
        ax.fill_between(x, fora, tot, step="post", color=AZUL, alpha=0.85, lw=0, label="Tomadas gerenciadas")
        teto(ax, 5080, "Disjuntor", x[0] + 1)
        ax.set_title(titulo, fontsize=8.5, color=TINTA, loc="left")
        ax.set_xlim(x[0], x[-1]); ax.set_xticks([1140, 1155]); ax.xaxis.set_major_formatter(hora_fmt)
        ax.tick_params(axis="x", labelsize=7.5)
    axs[0].set_ylabel("Potência da casa (W)"); axs[0].yaxis.set_major_formatter(milhar); axs[0].set_ylim(0, 8500)
    axs[0].legend(loc="upper center", ncol=2, bbox_to_anchor=(1.65, 1.32), fontsize=8)
    salvar(fig, "fig-cenario5-disjuntor")


# 4. Cenário 6: o conflito das 07:00 nas três decisões --------------------------------------
def fig_cenario6():
    ini, fim = passo_de(6, 35), passo_de(7, 15)
    ordem = ["geladeira", "ferro", "cafeteira", "microondas"]
    x = [(ini + i) * PASSO / 60 - DIA * 1440 for i in range(fim - ini)]
    fig, axs = plt.subplots(3, 1, figsize=(6.3, 4.6), sharex=True)
    for ax, (caso, titulo) in zip(axs, [("c6-regra", "Regra fixa"), ("c6-milp", "Modelo MILP"), ("c6-prioridade", "Regra de prioridade")]):
        jan = janela(caso, ini, fim)
        base = [0.0] * (fim - ini)
        for t in ordem:
            s = jan[t]
            topo = [b + (s[k][0] if s[k][1] != "OFF" else 0.0) for b, k in zip(base, range(ini, fim))]
            ax.fill_between(x, base, topo, step="post", color=COR[t], lw=0, label=NOME[t])
            base = topo
        teto(ax, 2540, "Teto de 2.540 W", x[0] + 0.5)
        ax.text(0.995, 0.92, titulo, transform=ax.transAxes, ha="right", va="top", fontsize=8.5, color=TINTA)
        ax.set_ylim(0, 3600); ax.yaxis.set_major_formatter(milhar)
    axs[1].set_ylabel("Potência das tomadas (W)")
    axs[-1].set_xlim(x[0], x[-1]); axs[-1].set_xticks(range(395, 436, 5)); axs[-1].xaxis.set_major_formatter(hora_fmt)
    axs[0].legend(loc="upper center", ncol=4, bbox_to_anchor=(0.5, 1.38), fontsize=8)
    salvar(fig, "fig-cenario6-conflito")


# 5. t_limite: economia x apertos -----------------------------------------------------------
def fig_t_limite():
    pts = []
    for t in (2, 5, 10, 20, 30, 60):
        with open(rodada(f"c4-pausas-t{t}") / "tomadas.csv", encoding="utf-8") as f:
            ts = list(csv.DictReader(f))
        eco = sum(float(x["economia_wh"]) for x in ts) / 1000
        apertos = sum(int(x["botoes_usar_agora"]) for x in ts if x["critica"] == "false") / 30
        pts.append((t, apertos, eco))
    fig, ax = plt.subplots(figsize=(4.6, 2.6))
    ax.plot([p[1] for p in pts], [p[2] for p in pts], color=AZUL, lw=1.2, alpha=0.5, zorder=2)
    ax.scatter([p[1] for p in pts], [p[2] for p in pts], s=36, color=AZUL, edgecolor="white", linewidth=1.5, zorder=3)
    # Rotulo por grupo de pontos coincidentes (t_limite abaixo da menor pausa cai no mesmo ponto).
    juntos = {}
    for t, a, e in pts:
        juntos.setdefault(round(a), []).append((t, a, e))
    # Rotulos alternando abaixo e acima, da esquerda para a direita, para nao encavalarem.
    for i, (_, grupo) in enumerate(sorted(juntos.items())):
        ts = [str(g[0]) for g in grupo]
        txt = (", ".join(ts[:-1]) + " e " + ts[-1] if len(ts) > 1 else ts[0]) + " min"
        ax.annotate(txt, (grupo[0][1], grupo[0][2]), xytext=(0, -15 if i % 2 == 0 else 9), textcoords="offset points",
                    ha="center", fontsize=8, color=TINTA2)
    # Eixo desde zero (revisao 4.10): a economia cai poucos por cento enquanto os acionamentos caem quase pela metade.
    p10, p60 = next(p for p in pts if p[0] == 10), next(p for p in pts if p[0] == 60)
    ax.text(0.5, 4.5, f"De 10 para 60 min: acionamentos {br(100 * (p60[1] / p10[1] - 1))}%,\n"
                      f"economia {br(100 * (p60[2] / p10[2] - 1))}%", fontsize=8, color=TINTA2)
    ax.set_xlabel("Acionamentos do “Usar Agora” por dia"); ax.set_ylabel("Economia nas tomadas (kWh/mês)")
    ax.set_xlim(0, 21); ax.set_ylim(0, 12); ax.set_xticks(range(0, 21, 5))
    ax.yaxis.set_major_formatter(FuncFormatter(lambda v, _: br(v, 0)))
    salvar(fig, "fig-t-limite")


# 6. As três placas reais -------------------------------------------------------------------
PAPEIS = {"ferro": (1000, 0.0, 20, 200), "air-fryer": (1500, 0.5, 60, 240), "microondas": (1400, 0.8, 120, 150)}


def fig_espnow():
    L = {}
    for porta in ("COM3", "COM7", "COM8"):
        for linha in (ESPNOW / f"serial-{porta}.txt").read_text(encoding="utf-8", errors="replace").splitlines():
            if "|L," in linha:
                c = linha.split("|", 1)[1].split(",")
                L.setdefault(c[1], []).append((int(c[2]), c[6]))
    ts = list(range(0, 1800))
    idx = {p: 0 for p in L}
    camadas = {p: [] for p in ("ferro", "air-fryer", "microondas")}
    for t in ts:
        for p in camadas:
            ls = L[p]
            while idx[p] + 1 < len(ls) and ls[idx[p] + 1][0] <= t:
                idx[p] += 1
            fechado = ls[idx[p]][0] <= t and ls[idx[p]][1] != "OFF"
            uso, esp, a, b = PAPEIS[p]
            camadas[p].append((uso if a <= t % 360 < b else esp) if fechado else 0.0)
    x = [t / 60 for t in ts]
    fig, ax = plt.subplots(figsize=(6.3, 2.5))
    ax.axvspan(20, 21.5, color=GRADE, alpha=0.7, lw=0, zorder=0)
    ax.text(20.75, 3700, "ferro sem\ntransmitir", ha="center", va="top", fontsize=7.5, color=TINTA2)
    base = [0.0] * len(ts)
    for p in ("ferro", "air-fryer", "microondas"):
        topo = [b + v for b, v in zip(base, camadas[p])]
        ax.fill_between(x, base, topo, step="post", color=COR[p], lw=0, label=NOME[p])
        base = topo
    teto(ax, 2540, "Teto de 2.540 W", 0.3)
    ax.set_xlim(0, 30); ax.set_xticks(range(0, 31, 5)); ax.set_xlabel("Tempo desde o início do teste (min)")
    ax.set_ylim(0, 4000); ax.set_ylabel("Potência (W)"); ax.yaxis.set_major_formatter(milhar)
    ax.legend(loc="upper center", ncol=3, bbox_to_anchor=(0.5, 1.2), fontsize=8)
    salvar(fig, "fig-espnow-3-placas")


if __name__ == "__main__":
    fig_espera_c4()
    fig_cenario5()
    fig_cenario6()
    fig_t_limite()
    fig_espnow()
    fig_energia()  # por último: varre os rastros inteiros
