"""Teste de ESP-NOW com tres placas (item 29, 2026-10-22): le COM3/COM7/COM8.txt e resume.

Linha do tempo comum pelo uptime (as placas reiniciam juntas): o rele de cada uma fica no
estado da ultima linha L ate a proxima; o aparelho puxa a demanda da rotina com o rele fechado.
Uso: python analise.py
"""
from pathlib import Path

DIR = Path(__file__).resolve().parent
P_MAX, PERIODO, DURACAO, MUDO = 2540, 360, 1800, (1200, 1290)
PAPEIS = {"ferro": (1000, 0.0, 20, 200), "air-fryer": (1500, 0.5, 60, 240), "microondas": (1400, 0.8, 120, 150)}


def demanda(papel, t):
    uso, espera, a, b = PAPEIS[papel]
    return uso if a <= t % PERIODO < b else espera


L, S, R, alinhamento = {}, {}, [], {}
for porta in ("COM3", "COM7", "COM8"):
    for linha in (DIR / f"serial-{porta}.txt").read_text(encoding="utf-8", errors="replace").splitlines():
        if "|" not in linha:
            continue
        ms, txt = linha.split("|", 1)
        c = txt.strip().split(",")
        if c[0] == "L" and len(c) == 13:
            papel, t = c[1], int(c[2])
            L.setdefault(papel, []).append(dict(t=t, estado=c[6], vivas=int(c[9]), carga=float(c[10]), ms_dec=int(c[11]), enviou=c[12] == "1"))
            alinhamento.setdefault(papel, []).append(int(ms) / 1000 - t)
        elif c[0] == "S":
            S[c[1]] = dict(enviados=int(c[4]), botoes=int(c[6]))
        elif c[0] == "R":
            R.append((c[1], c[2], int(c[4]), int(c[6])))

mediana = lambda v: sorted(v)[len(v) // 2]
print("alinhamento (PC - uptime, s):", {p: round(mediana(v), 2) for p, v in alinhamento.items()})
for p, ls in L.items():
    print(f"{p}: {len(ls)} ciclos, decidir mediana {mediana([x['ms_dec'] for x in ls])} ms, max {max(x['ms_dec'] for x in ls)} ms, "
          f"enviados {S.get(p, {}).get('enviados')}, botoes {S.get(p, {}).get('botoes')}")

# Segundo a segundo.
idx = {p: 0 for p in L}
acima, rajada, rajadas, pior = 0, 0, [], 0.0
negado = {p: 0 for p in L}
for t in range(30, DURACAO):
    total = 0.0
    for p, ls in L.items():
        while idx[p] + 1 < len(ls) and ls[idx[p] + 1]["t"] <= t:
            idx[p] += 1
        fechado = ls[idx[p]]["t"] <= t and ls[idx[p]]["estado"] != "OFF"
        d = demanda(p, t)
        total += d if fechado else 0.0
        negado[p] += d > 15 and not fechado
    if total > P_MAX:
        acima += 1; rajada += 1; pior = max(pior, total - P_MAX)
    elif rajada:
        rajadas.append(rajada); rajada = 0
print(f"acima do teto: {acima} s em {len(rajadas)} rajadas (maior {max(rajadas, default=0)} s, pior +{pior:.0f} W)")
print("uso negado (s):", negado)

print("enlaces (recebidos / perdidos):")
macs = {}
for p, m, rec, perd in R:
    macs.setdefault(m, set())
    print(f"  {p} <- {m}: {rec} recebidos, {perd} perdidos ({100 * perd / max(rec + perd, 1):.1f}%)")

print("falha do ferro (1200-1290 s), visto pelas outras:")
for p, ls in L.items():
    if p == "ferro":
        continue
    for x in ls:
        if MUDO[0] - 20 <= x["t"] <= MUDO[1] + 40:
            print(f"  {p} t={x['t']} vivas={x['vivas']} carga_fixa={x['carga']:.0f} estado={x['estado']}")
