"""Gera todos os .toml dos cenarios a partir de uma tabela so (2026-10-20).

Potencias: Inmetro/PBE (tabelas de 2024-2025), regulamentos da UE e Navigant 2020.
Horas, horarios e frequencia de uso: PPH 2019 da Eletrobras, relatorio de Roraima.
As fontes de cada numero estao em fontes-cenarios.md. Uso: python gerar-cenarios.py
"""
import os

# Aparelhos: (uso_w, standby_w, limiar_uso_w ou None). w = media do ciclo para os constantes.
GELADEIRA_1P = 31.5    # Inmetro 2025: refrigerador 1 porta 200-350 L, mediana 22,7 kWh/mes
GELADEIRA_FF = 55.6    # Inmetro 2025: refrigerador-congelador frost-free 350-480 L, 40,0 kWh/mes
FREEZER = 41.5         # Inmetro 2025: congelador horizontal 150-300 L, 29,9 kWh/mes
ROTEADOR = 10          # sem fonte (hipotese); PPH-RR: 86% ficam ligados 24 h
AP = {
    "tv-43":         (46, 0.3, None),   # Inmetro 2025, 39-44": 6,94 kWh/mes a 5 h/dia; espera mediana 0,3 W
    "tv-50":         (67, 0.3, None),   # Inmetro 2025, 49-51": 10,0 kWh/mes
    "tv-55":         (80, 0.3, None),   # Inmetro 2025, 55-56": 12,0 kWh/mes
    "tv-32":         (29, 0.3, None),   # Inmetro 2025, 24-33": 4,4 kWh/mes
    "decodificador": (11.0, 4.8, 8),    # Navigant 2020, media dos conformes 2018-2019
    "microondas":    (1400, 0.8, None), # Inmetro 2024: potencia nominal mediana; espera UE 2023/826 com mostrador
    "cafeteira":     (600, 0.5, None),  # uso sem tabela Inmetro (PROCEL); espera UE 2023/826
    "air-fryer":     (1500, 0.5, None), # uso sem tabela Inmetro (fabricante); espera UE 2023/826
    "notebook":      (45, 3.0, None),   # uso hipotese; suspensao UE 617/2013, Anexo II, 2.2 (notebook)
    "computador-3":  (150, 5.0, None),  # uso hipotese; suspensao UE 617/2013, Anexo II, 2.2 (de mesa)
    "computador-4":  (200, 5.0, None),
    "monitor":       (25, 0.5, None),   # uso hipotese; espera UE 2019/2021, Tabela 2
    "impressora":    (300, 2, None),    # uso hipotese; espera em rede UE 2023/826
    "console":       (150, 2, None),    # uso hipotese; espera em rede UE 2023/826
    "soundbar":      (30, 2, 10),       # uso hipotese; espera em rede UE 2023/826
    "home-theater":  (60, 0.8, None),   # uso hipotese; espera com mostrador UE 2023/826
    "carregador":    (18, 0.1, None),   # uso hipotese; sem carga UE 2019/1782
}
# Cargas fora do sistema (so entram na soma da casa e na conta do disjuntor).
AR_9 = 808      # Inmetro 2025: split hi-wall inverter 9.000 BTU/h, potencia a 35 C carga total, mediana
AR_12 = 1146    # idem, 12.000 BTU/h
LAMPADAS = 40   # PPH-RR: 6,1 lampadas por casa, 70% acesas as 20 h, LED de 9-11 W
LAVADORA = 360  # Inmetro 2022: 0,36 kWh/ciclo (10-13 kg), ciclo de 1 h
FERRO = 1000    # sem tabela Inmetro (PROCEL)
CHUVEIRO = 4400 # PPH: classe "< 4.500 W" e a mais citada; so no cenario 5 (0,96% das casas de RR tem)

# Janelas (PPH 2019, Roraima): horas com pelo menos metade do pico da tabela horaria.
J_TV = ["18:00-23:00"]           # Tab. 74: pico 50% as 20 h; 18-22 h acima de 25%
J_AR = ["21:00-06:00"]           # Tab. condicionador: pico 50% as 23 h; 21-05 h acima de 25%
J_LUZ = ["18:00-23:00"]          # Tab. lampadas: pico 70% as 20 h; 18-22 h acima de 35%
J_TV_QUARTO = ["20:00-22:00"]    # hipotese: TV secundaria, nas 2 horas de pico
J_DECO = ["18:00-23:00"]         # Tab. 198: uso modal 4-6 h por dia, junto com a TV
J_CONSOLE = ["19:00-22:00"]      # videogame: modal 2-4 h, pico as 19 h
J_NOTEBOOK = ["19:00-22:00"]     # notebook: modal 2-4 h, pico 19-21 h
J_PC = ["19:00-20:00"]           # computador: modal 31-60 min, pico 19-20 h
J_LAVADORA_FDS = ["09:00-10:00"] # maquinas ligadas: pico 8-10 h; 2-3 vezes por semana (modal)
J_FERRO_FDS = ["08:00-08:10"]    # ferro: ate 10 min (modal), pico 8 h; uma vez por semana ou menos


def grupo(nome, uso=None, w=None, janelas=None, critica=False, quantidade=1, defasagem=0,
          peso=10, extra=None):
    g = {"nome": nome}
    if quantidade > 1:
        g["quantidade"] = quantidade
        g["defasagem_min"] = defasagem
    if w is not None:
        g.update(perfil="constante", w=w)
        if critica:
            g["critica"] = True
        return g
    uso_w, standby_w, limiar = AP[uso] if isinstance(uso, str) else uso
    g.update(perfil="janelas", uso_w=uso_w, standby_w=standby_w)
    if limiar is not None:
        g["limiar_uso_w"] = limiar
    g["janelas"] = janelas
    if peso is not None:
        g["peso"] = peso
    g.update(extra or {})
    return g


def fora(nome, w, janelas, fds=None):
    g = {"nome": nome, "perfil": "janelas", "uso_w": w, "standby_w": 0, "janelas": janelas}
    if fds is not None:
        g["janelas_fds"] = fds
    return g


CENARIOS = {
    "c1-solteiro": dict(
        coment="Cenario 1: 1 morador, trabalha fora das 08:00 as 18:00. Casa vazia quase o dia todo.",
        descricao="1 morador que trabalha fora; muito standby ocioso",
        tomadas=[
            grupo("geladeira", w=GELADEIRA_1P, critica=True),
            grupo("roteador", w=ROTEADOR, critica=True),
            grupo("tv", "tv-43", janelas=J_TV),
            grupo("decodificador", "decodificador", janelas=J_DECO),
            grupo("microondas", "microondas", janelas=["07:00-07:05", "19:00-19:05"]),
            grupo("carregador-celular", "carregador", janelas=["23:00-00:00"]),
        ],
        fora=[fora("ar-quarto", AR_9, J_AR), fora("iluminacao", LAMPADAS, J_LUZ)],
    ),
    "c2-familia": dict(
        coment="Cenario 2: familia de 4 (2 adultos, 2 criancas). Uso de manha e a noite, casa nunca vazia a noite.",
        descricao="Familia de 4 pessoas; uso espalhado de manha e a noite",
        tomadas=[
            grupo("geladeira", w=GELADEIRA_FF, critica=True),
            grupo("roteador", w=ROTEADOR, critica=True),
            grupo("tv-sala", "tv-50", janelas=J_TV),
            grupo("decodificador", "decodificador", janelas=J_DECO),
            grupo("tv-quarto", "tv-32", janelas=J_TV_QUARTO, quantidade=2, defasagem=30),
            grupo("microondas", "microondas", janelas=["12:00-12:05", "19:00-19:05"]),
            grupo("cafeteira", "cafeteira", janelas=["06:30-06:40"]),
            grupo("carregador-celular", "carregador", janelas=["22:00-23:00"], quantidade=4, defasagem=20),
            grupo("notebook", "notebook", janelas=J_NOTEBOOK),
        ],
        fora=[fora("ar-casal", AR_12, J_AR), fora("ar-criancas", AR_9, J_AR),
              fora("iluminacao", LAMPADAS, J_LUZ),
              fora("maquina-lavar", LAVADORA, [], J_LAVADORA_FDS),
              fora("ferro", FERRO, [], J_FERRO_FDS)],
    ),
    "c3-home-office": dict(
        coment="Cenario 3: casal, um trabalha em casa das 08:00 as 18:00. Pouco tempo ocioso nos equipamentos de trabalho.",
        descricao="Home office; computador e monitores em uso o dia todo",
        tomadas=[
            grupo("geladeira", w=GELADEIRA_FF, critica=True),
            grupo("roteador", w=ROTEADOR, critica=True),
            # Horario de trabalho: hipotese do cenario (o PPH nao separa o home office).
            grupo("computador", "computador-3", janelas=["08:00-12:00", "13:00-18:00"]),
            grupo("monitor", "monitor", janelas=["08:00-12:00", "13:00-18:00"], quantidade=2),
            grupo("impressora", "impressora", janelas=["10:00-10:05", "16:00-16:05"]),
            grupo("tv", "tv-43", janelas=J_TV),
            grupo("microondas", "microondas", janelas=["12:00-12:05", "19:00-19:05"]),
            grupo("carregador-celular", "carregador", janelas=["23:00-00:00"], quantidade=2, defasagem=30),
        ],
        fora=[fora("ar-escritorio", AR_9, ["08:00-18:00"]), fora("ar-quarto", AR_12, J_AR),
              fora("iluminacao", LAMPADAS, J_LUZ)],
    ),
    "c4-eletronicos": dict(
        coment="Cenario 4: casal com sala de entretenimento. Muitos aparelhos em espera, usados poucas horas.",
        descricao="Sala cheia de eletronicos; consumo fantasma alto",
        tomadas=[
            grupo("geladeira", w=GELADEIRA_FF, critica=True),
            grupo("roteador", w=ROTEADOR, critica=True),
            grupo("tv", "tv-55", janelas=J_TV),
            grupo("decodificador", "decodificador", janelas=J_DECO),
            grupo("console", "console", janelas=J_CONSOLE),
            grupo("soundbar", "soundbar", janelas=J_TV),
            grupo("home-theater", "home-theater", janelas=J_CONSOLE),
            grupo("computador", "computador-4", janelas=J_PC),
            grupo("microondas", "microondas", janelas=["07:00-07:05", "19:00-19:05"]),
            grupo("carregador-celular", "carregador", janelas=["23:30-00:30"], quantidade=2, defasagem=30),
        ],
        fora=[fora("ar-quarto", AR_12, J_AR), fora("iluminacao", LAMPADAS, J_LUZ)],
    ),
    "c5-limite": dict(
        coment="Cenario 5: kitnet ligada como M1 (ate 4 kW, 40 A x 127 V = 5.080 W, NT/RRE-002) com mais carga instalada depois.",
        descricao="Caso limite: teto apertado e consumo dominado por cargas essenciais",
        tomadas=[
            grupo("geladeira", w=GELADEIRA_1P, critica=True),
            grupo("freezer", w=FREEZER, critica=True),
            grupo("roteador", w=ROTEADOR, critica=True),
            grupo("microondas", "microondas", janelas=["12:00-12:05", "19:00-19:05"]),
            # Fritadeira sem oleo: uso modal 31-60 min (PPH-RR); todo dia as 19 h e hipotese de estresse.
            grupo("air-fryer", "air-fryer", janelas=["19:00-19:45"]),
            grupo("tv", "tv-43", janelas=J_TV),
        ],
        # Banho de 6-10 min (modal PPH-RR), as 19 h (pico do PPH Brasil).
        fora=[fora("chuveiro", CHUVEIRO, ["19:00-19:10"]), fora("ar-quarto", AR_9, J_AR)],
    ),
}

# NT/RRE-002 v01, Tabela 07 (127 V): (limite kW, nome, disjuntor A, fases)
FAIXAS = [(4.0, "M1", 40, 1), (5.0, "M2", 50, 1), (7.5, "M3", 70, 1),
          (8.0, "B1", 40, 2), (10.0, "B2", 50, 2), (15.0, "B3", 60, 2)]


def carga_kw(c):
    t = 0.0
    for g in c["tomadas"] + c["fora"]:
        t += (g.get("w") or g.get("uso_w")) * g.get("quantidade", 1)
    return t / 1000


def disjuntor(c):
    kw = carga_kw(c)
    for lim, nome, a, fases in FAIXAS:
        if kw <= lim + 1e-9:
            return nome, a, fases, fases * 127 * a, kw
    raise ValueError(kw)


def valor(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, str):
        return f'"{v}"'
    if isinstance(v, list):
        return "[" + ", ".join(valor(x) for x in v) + "]"
    if isinstance(v, float) and v.is_integer():
        return str(int(v))
    return str(v)


def toml(nome, coment, descricao, p_max, arquivo, grupos, extra_base=None, reserva=None):
    linhas = [f"# {l}" for l in coment.split("\n")]
    linhas += [f'nome = "{nome}"']
    if descricao:
        linhas += [f'descricao = "{descricao}"']
    linhas += ['duracao = "30d"', "passo_s = 10", "[base]", f'arquivo = "{arquivo}"', "passo_s = 10"]
    for k, v in (extra_base or {}).items():
        linhas.append(f"{k} = {valor(v)}")
    linhas += ["[casa]", f"p_max_w = {p_max}"]
    for janela, w in reserva or []:
        linhas += ["[[casa.reserva]]", f'janela = "{janela}"', f"w = {w}"]
    for g in grupos:
        linhas.append("[[grupo]]")
        linhas += [f"{k} = {valor(v)}" for k, v in g.items()]
    return "\n".join(linhas) + "\n"


def escreve(arq, texto):
    with open(arq, "w", encoding="utf-8", newline="\n") as f:
        f.write(texto)


def com(grupos, **mudancas):
    """Copia dos grupos nao essenciais com campos trocados (por nome do grupo ou para todos)."""
    out = []
    for g in grupos:
        g = dict(g)
        if g.get("perfil") == "janelas":
            for k, v in mudancas.items():
                v = v(g) if callable(v) else v
                if v is not None:
                    g[k] = v
        out.append(g)
    return out


FORA_COMENT = "Cargas FORA do sistema (sem tomada inteligente): o MILP nunca as ve. So para gerar-base."

if __name__ == "__main__":
    resumo = []
    for nome, c in CENARIOS.items():
        f, a, fases, pmax, kw = disjuntor(c)
        if nome == "c5-limite":
            f, a, fases, pmax = "M1", 40, 1, 5080  # subdimensionado de proposito
        resumo.append(f"{nome}: carga instalada {kw:.2f} kW -> {f}, {fases} x 127 V x {a} A = {pmax} W")
        disj = (f"Disjuntor {a} A {'bifasico, 2 x ' if fases == 2 else 'x '}127 V"
                f"{'' if fases == 1 else ' x ' + str(a) + ' A'} = {pmax} W: faixa {f} "
                f"(carga instalada {kw:.1f} kW), Tabela 07 da NT/RRE-002 (Roraima Energia).")
        escreve(f"{nome}-fora.toml", toml(f"{nome}-fora", FORA_COMENT, None, 100000,
                                          f"{nome}-fora.csv", c["fora"]))
        if nome != "c5-limite":
            escreve(f"{nome}.toml", toml(nome, c["coment"] + "\n" + disj, c["descricao"], pmax,
                                         f"{nome}.csv", c["tomadas"]))
            continue
        base = "c5-limite.csv"  # gerada pelo gerar-base do c5a-cego
        escreve("c5a-cego.toml", toml("c5a-cego", c["coment"] + f"\nCarga instalada {kw:.1f} kW. Chuveiro e ar fora do sistema; MILP ve P_max = disjuntor.",
                                      c["descricao"], 5080, base, c["tomadas"]))
        escreve("c5b-reserva.toml", toml("c5b-reserva", f"Cenario 5b: igual ao 5a, mas o usuario reserva o chuveiro: P_max = 5.080 - {CHUVEIRO} = {5080 - CHUVEIRO} W.",
                                         c["descricao"], 5080 - CHUVEIRO, base, c["tomadas"]))
        escreve("c5c-reserva-horario.toml", toml("c5c-reserva-horario", f"Cenario 5c: disjuntor inteiro (5.080 W) como teto, com o chuveiro ({CHUVEIRO} W) reservado so das 19:00 as 19:10, quando ele liga.",
                                                 c["descricao"], 5080, base, c["tomadas"], reserva=[("19:00-19:10", CHUVEIRO)]))

    c4 = CENARIOS["c4-eletronicos"]
    pmax4 = disjuntor(c4)[3]
    for w in (1, 100):
        escreve(f"c4-w{w}.toml", toml(f"c4-w{w}", f"Cenario 4 com W_i = {w} em todos os aparelhos nao essenciais (varredura de W_i; o c4 original usa 10).",
                                      c4["descricao"], pmax4, "c4-eletronicos.csv", com(c4["tomadas"], peso=w)))
    sorteio = dict(variacao_min=lambda g: 15 if g["nome"] == "microondas" else 45 if g["nome"].startswith("carregador") else 30,
                   variacao_duracao=lambda g: 0.5 if g["nome"] == "microondas" else 0.3)
    fds = {  # hipotese: o casal fica em casa sabado e domingo
        "tv": ["10:00-12:00", "14:00-23:00"], "decodificador": ["10:00-12:00", "18:00-23:00"],
        "console": ["14:00-18:00", "19:00-22:00"], "soundbar": ["14:00-23:00"],
        "home-theater": ["19:00-22:00"], "computador": ["10:00-12:00"],
        "microondas": ["09:00-09:05", "12:00-12:05", "19:00-19:05"],
    }
    for s in (1, 2, 3):
        escreve(f"c4-sorteio-s{s}.toml", toml(f"c4-sorteio-s{s}", f"Cenario 4 com rotina sorteada (semente {s}): cada dia, inicio de cada uso +-30 min (micro-ondas +-15, carregadores +-45) e duracao +-30% (micro-ondas +-50%). Mesmas janelas de semana do c4.",
                                              c4["descricao"], pmax4, f"c4-sorteio-s{s}.csv", com(c4["tomadas"], **sorteio), {"semente": s}))
        escreve(f"c4-fds-s{s}.toml", toml(f"c4-fds-s{s}", f"Cenario 4 com rotina sorteada (semente {s}) e fim de semana proprio: o casal fica em casa sabado e domingo (TV e console a tarde, almoco). Sorteio igual ao c4-sorteio.",
                                          c4["descricao"], pmax4, f"c4-fds-s{s}.csv", com(c4["tomadas"], janelas_fds=lambda g: fds.get(g["nome"]), **sorteio), {"semente": s}))
    # Achado 4.2 do orientador (2026-10-27): sorteio nos Cenarios 1-4, sementes 1-30, em sorteio/.
    os.makedirs("sorteio", exist_ok=True)
    for nome in ("c1-solteiro", "c2-familia", "c3-home-office", "c4-eletronicos"):
        c = CENARIOS[nome]
        for s in range(1, 31):
            escreve(f"sorteio/{nome}-s{s}.toml", toml(f"{nome}-s{s}", f"{c['coment']}\nRotina sorteada (semente {s}), mesmo sorteio do c4-sorteio.",
                                                     c["descricao"], disjuntor(c)[3], f"{nome}-s{s}.csv", com(c["tomadas"], **sorteio), {"semente": s}))
        if nome == "c4-eletronicos":
            for s in range(1, 31):
                escreve(f"sorteio/c4-fds-s{s}.toml", toml(f"c4-fds-s{s}", f"Cenario 4 com rotina sorteada (semente {s}) e fim de semana proprio.",
                                                         c["descricao"], pmax4, f"c4-fds-s{s}.csv", com(c["tomadas"], janelas_fds=lambda g: fds.get(g["nome"]), **sorteio), {"semente": s}))
    print("\n".join(resumo))
