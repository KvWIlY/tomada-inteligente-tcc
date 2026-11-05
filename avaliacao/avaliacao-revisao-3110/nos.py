"""Porta da busca de firmware/src/solver/solver.rs, com as instancias Pior de examples/bancada.rs, contando nos."""
import sys, statistics as st
sys.setrecursionlimit(100)
EPS, LIM, M = 1e-6, 200_000, (1 << 64) - 1
def sort(s):
    s[0] = (s[0] + 0x9e3779b97f4a7c15) & M
    z = s[0]
    z = ((z ^ (z >> 30)) * 0xbf58476d1ce4e5b9) & M
    z = ((z ^ (z >> 27)) * 0x94d049bb133111eb) & M
    return ((z ^ (z >> 31)) >> 11) / (1 << 53)
def inst(n, s):
    sort(s)  # giro
    t = [(1000.0, 10.0)] + [(1000.0 + sort(s), 10.0 if j % 2 == 0 else 3.0) for j in range(1, n)]
    return t, sum(p for p, _ in t) / 2
def nos(t, teto):
    op = [(p, 0.5 * w + 1e6 - p) for p, w in t]
    # gulosa
    usado = g = 0.0
    for i in sorted(range(len(op)), key=lambda i: (-op[i][1], i)):
        if usado + op[i][0] <= teto + EPS:
            usado += op[i][0]; g += op[i][1]
    resto = [0.0] * (len(op) + 1)
    for k in range(len(op) - 1, -1, -1):
        resto[k] = resto[k + 1] + max(op[k][1], 0.0)
    melhor, cont = [g - 2 * EPS], [0]
    def dfs(k, p, gan):
        cont[0] += 1
        if cont[0] > LIM or gan + resto[k] <= melhor[0] + EPS:
            return
        if k == len(op):
            melhor[0] = gan
            return
        o = op[k]
        if p + o[0] <= teto + EPS:
            dfs(k + 1, p + o[0], gan + o[1])
        dfs(k + 1, p, gan)
    dfs(0, 0.0, 0.0)
    return cont[0]
for n, rep in ((6, 1000), (10, 1000), (13, 1000), (16, 100), (17, 100), (20, 10)):
    s = [n * 1000 + 3]
    v = [nos(*inst(n, s)) for _ in range(rep)]
    print(n, 'rep', rep, 'primeira', v[0], 'mediana', int(st.median(v)), 'max', max(v), 'limite', sum(x > LIM for x in v), 'teto arvore', 2 ** (n + 1) - 1)
