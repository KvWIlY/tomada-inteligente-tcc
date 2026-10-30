# `frontend/` - dashboard do dono

Vite + TypeScript 7 + Tailwind 4 + Basecoat UI. **Sem framework**: as views são funções
TypeScript que devolvem marcação como string, e o `main.ts` decide qual delas renderizar. Não há
React, nem Vue, nem build de componentes - foi decisão de arquitetura, preservada inteira no
redesign da interface.

## Subir

```powershell
cd C:\y\frontend
copy .env.example .env       # VITE_BACKEND_URL=http://localhost:8080
npm install
npm run dev                  # :5173
```

O backend precisa estar no ar em `:8080` - e com a origem do dashboard em `CORS_ORIGINS`
(`backend/.env`), senão toda requisição falha com um erro que *parece* de autenticação.

| Comando | O que faz |
|---|---|
| `npm run dev` | Servidor de desenvolvimento com HMR |
| `npm run build` | `tsc && vite build` → `dist/` |
| `npm run preview` | Serve o `dist/` já construído |

**`npm run build` é a única verificação que existe** - e build verde não prova nada sobre a
tela: três defeitos visuais reais passaram por `tsc && vite build` sem um pio, incluindo um que
apagava dados válidos quando a rede caía.

## Configuração (`.env`)

| Variável | Efeito |
|---|---|
| `VITE_BACKEND_URL` | Base da API, **sem barra final** |
| `VITE_RESIDENCIA_ID` | Opcional: fixa uma casa em vez de usar a primeira do dono |

**Não existe modo MOCK.** Sem `VITE_BACKEND_URL` o dashboard mostra uma tela dizendo o que falta
configurar e para por ali. O gerador de dados falsos foi removido de propósito: ele mascarava
exatamente as falhas de integração que só aparecem contra a API real.

## Estrutura

```
src/
├── main.ts              # roteamento, polling, estado da barra lateral, seletor de tema
├── store.ts             # store minimalista: get / set / subscribe
├── types.ts             # o contrato com o backend (Tomada, Medicao, Evento, Config…)
├── derive.ts            # dados derivados das medições reais (séries, agregados)
├── utils.ts             # helpers de formatação (tempo relativo, números)
├── icons.ts             # os 16 Material Symbols, inlinados
├── services/api.ts      # cliente HTTP + dono da sessão
├── styles/
│   ├── app.css          # folha raiz - a ORDEM dos @import importa
│   └── tokens.css       # paleta e as poucas regras que o Basecoat não cobre
└── views/
    ├── login.ts         ├── overview.ts    ├── detail.ts
    ├── sidebar.ts       ├── charts.ts      ├── components.ts   └── toast.ts
```

`store.ts`, `derive.ts`, `utils.ts`, `types.ts` e `services/api.ts` **não foram tocados** no
redesign - ele mexeu só na camada visual, e `main.ts` entrou nessa lista por decisão explícita
do dono.

## Sessão e dados

`services/api.ts` é o **dono da sessão**: guarda o JWT de usuário, o `exp` e o e-mail (só para
exibir) em `localStorage`, sob `tcc.sessao`. Nunca a senha. Um token expirado é tratado como
deslogado na leitura, não em um timer.

O `main.ts` faz **polling a cada 15 s** (`INTERVALO_POLL_MS`). Uma armadilha herdada:
`services/api.ts` engole erro de rede e devolve lista vazia - a guarda contra escrever esse
resultado degradado no estado vive no `main.ts`. Quem mexer no polling precisa manter a guarda,
ou uma queda de rede momentânea apaga a tela.

`Medicao.fator_potencia` é `number | null`. **`null` significa desconhecido e nunca deve virar 0
num gráfico** - vem de medições anteriores a 2026-08-24, quando a potência ainda era aparente.

## A pilha visual

**Basecoat UI (`basecoat-css` 1.0.2) sobre Tailwind 4.** CSS puro, sem JavaScript de componente
(com uma exceção, abaixo). O `legacy.css` foi apagado: suas 465 linhas viraram componentes do
Basecoat mais utilitários do Tailwind consumindo os **mesmos** tokens.

A ordem em `styles/app.css` importa e não é acidental:

```css
@import "tailwindcss";   /* preflight + utilitários */
@import "basecoat-css";  /* componentes */
@import "./tokens.css";  /* nosso, fora de @layer - por isso vence os dois */
```

Quatro coisas que custaram uma iteração cada:

- **Os tokens chegam ao Tailwind por `@theme inline`.** Sem o `inline`, os utilitários
  congelariam o hexadecimal do tema claro e o modo escuro não alcançaria nenhum deles.
- **A barra lateral usa a CSS do `sidebar` do Basecoat, mas NÃO o JS dele.** O `handleClick` da
  biblioteca fecha a barra quando o clique não cai dentro do `<nav>` - e as views reescrevem
  `innerHTML` no próprio handler, então o botão clicado fica desanexado e o componente conclui
  que o clique foi fora. No desktop a barra sumia até um F5. O estado é do `main.ts`: fixa acima
  de 768 px, gaveta abaixo.
- **Tema claro/escuro por seletor** (botão sol/lua), classe `dark` no `<html>`, persistido em
  `localStorage` sob `themeMode` - a mesma chave que o `basecoat.theme.set()` já usa, não
  inventamos outra. **O padrão é o claro**: a ausência de escolha *não* consulta
  `prefers-color-scheme`, por decisão do dono. A classe é
  aplicada por um **script inline no `<head>`**, antes da primeira pintura - senão quem escolhe
  escuro vê um lampejo branco a cada carregamento.
- **Contraste: medir, nunca estimar.** Uma estimativa de borda de campo errou por um terço. Os
  tokens de estado têm dois papéis distintos: a variante base para fundo e ornamento, a `-ink`
  para texto, e `--amber-line` para gráfico que carrega significado.

## Ícones e fonte - ambos locais

**Material Symbols** (Google, Apache-2.0), estilo *outlined*, vendorizados em `public/icons/`
com o texto integral da licença e o mapa nome-no-código → nome-no-set em `ORIGEM.md`. São **16**,
só os efetivamente usados. Os corpos estão inlinados em `src/icons.ts`, porque as views montam
marcação de forma síncrona e não podem esperar uma requisição - e por isso o pacote npm **não** é
dependência declarada.

São **formas preenchidas, não traços** (`fill="currentColor"`, sem `stroke-width`): na migração
saindo do Lucide, o `icon()` perdeu o parâmetro de espessura, porque manter um argumento que não
faz nada é convidar o próximo a acreditar nele.

O favicon (`public/favicon.svg`) é o mesmo `bolt` da marca do cabeçalho, com a cor **literal** -
a aba do navegador não resolve `var()` nem herda cor.

**Open Sans local** em `public/fonts/`, só os subsets `latin` e `latin-ext`: 6 arquivos (~243 kB)
em vez de 30. Até o redesign, o `index.html` buscava DM Sans no `fonts.googleapis.com` - numa casa sem
WAN, que é o cenário do projeto, a fonte simplesmente não carregava.

## O que saiu da tela e continua no contrato

Agendamento operacional, "Probabilidade de Uso" e o `build7d` saíram da interface. O seletor de
7 dias rebusca a série real do backend (`janela=7d`) em vez de reconstruí-la no cliente. Mas
`config.schedule` **continua intacto** no contrato e no banco - não remova achando que é código
morto. O `config.pi_historico` saiu do sistema inteiro em 2026-09-14, porque nada o calculava.
