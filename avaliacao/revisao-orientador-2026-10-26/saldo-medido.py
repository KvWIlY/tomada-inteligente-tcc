"""Saldo de cada cenario com o consumo MEDIDO da tomada (revisao do orientador 3.4, 2026-10-27).

Mesma conta do `parecer-revisor-2026-10-22/02-apertos-rele-autoconsumo/autoconsumo.py`, trocando
as folhas de dados pelo medidor USB (protocolo em `consumo-proprio.md`). Entradas em mW, no lado
de 5 V, medias de 10 min:

  MODEM_ABERTO, MODEM_FECHADO  firmware de producao conectado ao roteador (modem-sleep do ESP-IDF)
  RADIO_ABERTO                 exemplo `espnow_teste` (radio sempre recebendo, rele aberto)

O fechado com o radio sempre recebendo nao e medido: soma a RADIO_ABERTO o que a bobina acrescentou
no modem-sleep (MODEM_FECHADO - MODEM_ABERTO). Ficam fora do medidor e seguem estimados: a fonte
AC/DC, que ainda nao existe (rendimento minimo do Regulamento (UE) 2019/1782 a 10% de carga, 0,688),
e o ZMPT101B, ligado direto na rede (1-2 mA a 127 V, folha de dados).

Uso: python saldo-medido.py MODEM_ABERTO MODEM_FECHADO RADIO_ABERTO [pasta das rodadas]
"""
import csv
import math
import sys
from pathlib import Path

ROD = Path(sys.argv[4]) if len(sys.argv) > 4 else Path(__file__).resolve().parent.parent / "avaliacao-busca-exata" / "rodadas"
ETA = 0.0834 * math.log(5) - 0.0011 * 5 + 0.559
REDE, ZMPT_A = 127.0, (0.001, 0.002)
CENARIOS = ("c1-solteiro", "c2-familia", "c3-home-office", "c4-eletronicos")

m_ab, m_fe, r_ab = (float(x) / 1000 for x in sys.argv[1:4])
MODOS = {
    "radio sempre recebendo (teste de coordenacao)": (r_ab, r_ab + (m_fe - m_ab)),
    "modem-sleep (firmware conectado ao roteador)": (m_ab, m_fe),
}


def tomadas(caso):
    pasta = sorted((ROD / caso).glob("2*"))[-1]
    with open(pasta / "tomadas.csv", encoding="utf-8") as f:
        for t in csv.DictReader(f):
            total = int(t["tempo_off_s"]) + int(t["tempo_std_s"]) + int(t["tempo_on_s"])
            fechado = (int(t["tempo_std_s"]) + int(t["tempo_on_s"])) / total
            yield t["critica"] == "true", fechado, float(t["economia_wh"]) / 1000


br = lambda x: f"{x:+.2f}".replace(".", ",")
print(f"rodadas: {ROD}\nrendimento da fonte: {ETA:.3f}")
for nome, (ab5, fe5) in MODOS.items():
    # Na parede: 5 V / rendimento + ZMPT101B (faixa da folha de dados).
    parede = [(ab5 / ETA + REDE * z, fe5 / ETA + REDE * z) for z in ZMPT_A]
    (a0, f0), (a1, f1) = parede
    print(f"\n{nome}: 5 V aberto {ab5:.3f} W, fechado {fe5:.3f} W | "
          f"parede aberto {a0:.2f}-{a1:.2f} W, fechado {f0:.2f}-{f1:.2f} W")
    print("cenario | economia | consumo das tomadas (kWh/mes) | saldo com criticas | saldo sem criticas")
    for c in CENARIOS:
        ts = list(tomadas(c))
        eco = sum(e for _, _, e in ts)
        gasto = lambda sel, ab, fe: sum(ab + (fe - ab) * fr for cr, fr, _ in ts if sel(cr)) * 720 / 1000
        todas = (gasto(lambda cr: True, a0, f0), gasto(lambda cr: True, a1, f1))
        sem = (gasto(lambda cr: not cr, a0, f0), gasto(lambda cr: not cr, a1, f1))
        print(f"{c} | {eco:.2f} | {todas[0]:.2f}-{todas[1]:.2f} | "
              f"{br(eco - todas[1])} a {br(eco - todas[0])} | {br(eco - sem[1])} a {br(eco - sem[0])}")
