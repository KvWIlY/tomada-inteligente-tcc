"""C1-C4 com espera de aparelhos antigos: media da Tabela 1 de Ross e Meier (LBNL-45967, 2000).
So troca standby_w e, no limite superior, o limiar do decodificador. Aparelho sem linha na tabela
ou com valor atual maior fica como esta (computador 5 W, console 2 W, carregador 0,1 W)."""
import re
from pathlib import Path
ORIG = Path("../avaliacao-busca-exata")
ROSS = {"tv": 6.4, "decodificador": 10.2, "microondas": 2.8, "notebook": 4.5, "monitor": 2.0,
        "impressora": 4.2, "soundbar": 2.8, "home-theater": 2.8}
casos = []
for c in ("c1-solteiro", "c2-familia", "c3-home-office", "c4-eletronicos"):
    t = (ORIG / f"{c}.toml").read_text(encoding="utf-8")
    for var in ("orig", "legado", "legado-corta-deco"):
        nome = f"{c}-{var}"
        blocos = t.split("[[grupo]]")
        for i, b in enumerate(blocos[1:], 1):
            g = re.search(r'nome = "([^"]+)"', b).group(1)
            ap = next((k for k in ROSS if g == k or g.startswith(k + "-")), None)
            if var != "orig" and ap and "standby_w" in b:
                b = re.sub(r"standby_w = [\d.]+", f"standby_w = {ROSS[ap]}", b)
                if ap == "decodificador" and var == "legado-corta-deco":
                    b = re.sub(r"limiar_uso_w = [\d.]+", "limiar_uso_w = 10.6", b)
            blocos[i] = b
        s = "[[grupo]]".join(blocos)
        s = re.sub(r'nome = "c\d-[^"]*"', f'nome = "{nome}"', s, count=1)
        s = re.sub(r'arquivo = "[^"]*"', f'arquivo = "{nome}.csv"', s, count=1)
        Path(f"{nome}.toml").write_text(s, encoding="utf-8", newline="\n")
        casos.append(nome)
Path("casos-legado.txt").write_text("\n".join(casos) + "\n", encoding="utf-8", newline="\n")
