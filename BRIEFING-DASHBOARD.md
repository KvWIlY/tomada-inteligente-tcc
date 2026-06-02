# Briefing — Dashboard de Tomadas Inteligentes

> Documento para alimentar o **Claude (designer)**. Contém (1) o que é o projeto e
> como ele funciona, e (2) especificações concretas do dashboard a ser desenhado.
> Copie a seção **"Prompt pronto para o designer"** no final se quiser ir direto ao ponto.

---

## 1. O que é o projeto

Sistema de **otimização energética de tomadas inteligentes** para uma residência. Cada
tomada é um dispositivo **ESP32** (firmware em Rust) que mede o consumo do aparelho ligado
nela e decide automaticamente se deve ficar **ligada, em standby ou desligada**, de forma a
**minimizar o consumo total sem estourar a capacidade elétrica da casa** (`P_max`).

É um TCC. O núcleo é um **modelo de otimização matemática (MILP — programação linear
inteira mista)** que resolve um problema do tipo "mochila": dado um teto de potência, quais
aparelhos manter ligados para gastar o mínimo, respeitando conforto e regras do usuário.

A nuvem é o **Supabase** (banco Postgres + autenticação + funções). O dashboard web é a
**interface do dono da casa**: é por onde ele vê o consumo, controla as tomadas e configura
as regras.

### Em uma frase
Tomadas que se auto-organizam para economizar energia sob um limite elétrico, com um
dashboard onde o dono acompanha e comanda tudo.

---

## 2. Como funciona (visão geral)

### Os três estados de cada tomada
| Estado | O que significa | Relé físico |
|--------|-----------------|-------------|
| **OFF** (desligada) | energia cortada, consumo zero | aberto |
| **STD** (standby) | energizada, mas em consumo "fantasma" (aparelho parado) | fechado |
| **ON** (ligada) | energizada e em uso ativo | fechado |

> Curiosidade de hardware: o relé só abre ou fecha. A diferença entre **STD** e **ON** não é
> atuada — é **inferida pelos sensores** (consumo baixo = fantasma; consumo alto = uso ativo).

### O ciclo de cada tomada (a cada poucos segundos)
1. **Medir** — sensores de tensão (ZMPT101B) e corrente (SCT-013) calculam a potência (W).
2. **Decidir** — o solver MILP escolhe o estado de todas as tomadas ao mesmo tempo
   (decisão **global**, não tomada a tomada), respeitando o teto `P_max`.
3. **Atuar** — abre/fecha o relé conforme a decisão.
4. **Sincronizar** — envia medições e eventos para o Supabase em lote.

### O que o modelo leva em conta
- **Teto elétrico `P_max`** — a soma das potências nunca pode passar disso. Regra inviolável.
- **Conforto preditivo** — cada tomada tem uma **probabilidade histórica de uso por hora do
  dia** (`Π`). Desligar um aparelho que costuma ser usado naquele horário é penalizado.
- **Uso ativo** — desligar algo que está em uso agora custa caro (penalidade).
- **Agendamento** — horários permitidos por tomada (ex.: TV só à noite).
- **Criticidade** — geladeira, roteador etc. são marcados como "não pode desligar".
- **Comando manual** — o dono liga/desliga pelo dashboard; é respeitado, **exceto** se
  violar o teto elétrico (a segurança elétrica sempre vence).

### Coordenação entre tomadas
As tomadas conversam entre si via **ESP-NOW** (rádio direto, sem Wi-Fi) para somar o
consumo de todas e avaliar o teto global de forma coordenada. Se uma tomada some da rede,
o sistema entra num modo local seguro que não viola o teto.

### Resiliência (offline-first)
Sem internet, a tomada continua medindo e decidindo. Os dados ficam guardados localmente e
**reconciliam ao reconectar**, sem duplicar nada. Dados confirmados na nuvem são apagados do
dispositivo após até 7 dias (proteção da memória flash).

### Segurança (o que o dashboard precisa respeitar)
- Cada tomada tem **identidade própria** (JWT por dispositivo) e só enxerga dados da
  **própria residência**. Isolamento total entre casas (multi-inquilino).
- Algumas colunas são **só do dono** (comando manual do relé, teto `P_max`, identidade de
  hardware). O dispositivo não pode escrevê-las — **o dashboard sim**.

---

## 3. Dados que o dashboard exibe e edita

Tabelas principais no Supabase (nomes reais):

### `tomadas` — uma linha por tomada
| Campo | Tipo | Quem edita | Uso no dashboard |
|-------|------|------------|------------------|
| `id` | uuid | sistema | identificador |
| `mac` | texto | sistema | identidade de hardware (só leitura) |
| `residencia_id` | uuid | sistema | agrupa as tomadas da casa |
| `estado` | OFF/STD/ON | tomada | **badge de status atual** |
| `rele_aberto` | bool | **dono (dashboard)** | **toggle ligar/desligar manual** |
| `p_medida_atual` | float (W) | tomada | **consumo agora** |
| `is_critical` | bool | **dono** | marca "não pode desligar" |
| `p_max` | int (W) | **dono** | teto elétrico da casa |
| `schedule` | bool[24] | **dono** | **agendamento por hora (grade 24h)** |
| `pi_historico` | float[24] | sistema | probabilidade de uso por hora (gráfico) |
| `t_limite` | int (s) | dono | limite de ociosidade |
| `config_version` | int | sistema | controle de sincronização |

### `medicoes` — histórico de consumo (1 registro/minuto por tomada)
`tomada_id`, `timestamp`, `p_medida` (W), `estado`, `rele_aberto`.
→ Alimenta **gráficos de consumo ao longo do tempo**.

### `eventos` — observabilidade / log
`tomada_id`, `timestamp`, `tipo` (chaveamento, penalidade_uso, penalidade_agenda,
fantasma_eliminado, solver), `severidade` (info/warn/erro), `titulo`, `detalhes`.
→ Alimenta uma **timeline / feed de eventos**.

---

## 4. Especificação do dashboard (para o designer)

### Público e objetivo
Dono de uma casa, não-técnico. Precisa **entender o consumo de relance**, **agir rápido**
(ligar/desligar) e **confiar** que o sistema está economizando sem deixá-lo na mão.

### Princípios de design
- **Clareza acima de tudo** — números grandes, estados óbvios por cor.
- **Tempo real** — sensação de que os dados estão vivos (valores que atualizam).
- **Controle com segurança** — comandos manuais visíveis, mas o teto elétrico sempre claro.
- **Mobile-first** — o dono vai abrir no celular. Deve escalar bem para desktop.

### Estrutura geral — simples, com barra lateral

É um **dashboard simples**: uma **barra lateral fixa à esquerda** que faz toda a navegação,
e uma **área de conteúdo à direita** que troca conforme o que está selecionado na barra.

```
┌──────────────┬─────────────────────────────────────┐
│  SIDEBAR     │  CONTEÚDO                            │
│              │                                      │
│ ▸ Visão geral│   (muda conforme o botão clicado)    │
│              │                                      │
│ Tomadas      │                                      │
│ ▸ Tomada 1   │                                      │
│ ▸ Tomada 2   │                                      │
│ ▸ Geladeira  │                                      │
│ ▸ ...        │                                      │
└──────────────┴─────────────────────────────────────┘
```

**Barra lateral (navegação):**
- No topo, um botão **"Visão geral"**.
- Abaixo dele, **um botão para cada tomada** (lista). Cada botão mostra o nome da tomada e
  uma bolinha/ponto colorido com o estado atual (verde ON, âmbar STD, cinza OFF).
- Clicar em "Visão geral" → mostra o **resumo da casa**.
- Clicar em uma tomada → mostra os **dados daquela tomada**.
- No celular, a barra lateral vira um menu recolhível (hambúrguer) ou abas no rodapé.

**Conteúdo — "Visão geral" (dados da casa):**
- **Consumo total da casa agora** (W grande) e o **teto `P_max`** como **barra de
  capacidade** (ex.: 1.840 W / 3.000 W — 61%). A cor muda perto do limite.
- Lista/grid compacto com **todas as tomadas** e seu estado + consumo, para ver tudo de
  relance (resumo do que está na barra lateral, mas com mais dado).
- Indicador de **status de conexão** (online / sincronizando / offline).
- Opcional: editar o teto `P_max` da casa aqui mesmo.

**Conteúdo — "Tomada selecionada" (dados de uma tomada):**
- Cabeçalho: nome, **estado atual** (badge colorido), **consumo agora** (W) e o
  **toggle ligar/desligar manual**. Selo "crítica" se for o caso.
- **Gráfico de consumo** (últimas 24h / 7 dias) a partir de `medicoes`.
- **Probabilidade de uso por hora** (`pi_historico` — 24 barras).
- **Agendamento**: grade de 24 horas clicável (permitido/proibido) → edita `schedule`.
- Configurações da tomada: marcar como crítica, limites.

> Eventos/log e pareamento de novas tomadas podem ser uma aba extra na barra lateral ou
> ficar dentro da "Visão geral" — opcional, manter simples.

### Linguagem visual sugerida
- **Tema**: energia / sustentabilidade. Base clara e limpa, com suporte a dark mode.
- **Cores de estado**: ON = verde, STD = âmbar/amarelo, OFF = cinza neutro;
  alerta de teto = laranja→vermelho.
- **Tipografia**: números de consumo com destaque (peso alto, tabulares).
- **Componentes**: cards arredondados, badges de estado, toggles, barra de capacidade,
  gráficos de linha (consumo) e de barras (probabilidade/agenda).
- **Tom**: confiável e calmo, não "gamer". Pense em app de energia residencial moderno.

### Estados de interface a não esquecer
- **Offline / sincronizando** — indicar claramente, sem assustar (o sistema funciona offline).
- **Comando manual recusado pelo teto** — feedback explicando que ligaria acima do `P_max`.
- **Tomada crítica** — toggle de desligar deve avisar/confirmar.
- **Sem dados ainda** (tomada recém-pareada) — estado vazio amigável.

### Stack sugerida (se o designer gerar código)
React + TypeScript + Tailwind, com Supabase JS para os dados em tempo real (Realtime nas
tabelas `tomadas` e `medicoes`). Componentes acessíveis (shadcn/ui é uma boa base).

---

## 5. Prompt pronto para o designer

> Cole isto no Claude quando for pedir o design:

```
Desenhe um dashboard web SIMPLES (mobile-first, com dark mode) para um sistema de tomadas
inteligentes residenciais que otimizam o consumo de energia sob um teto elétrico (P_max).

Contexto: cada tomada (dispositivo ESP32) mede o consumo do aparelho e fica em um de três
estados — ON (em uso, verde), STD (standby/consumo fantasma, âmbar) ou OFF (desligada,
cinza). Um otimizador decide os estados globalmente para minimizar o consumo sem estourar
o P_max da casa. O dono usa o dashboard para acompanhar e comandar.

Estrutura: uma BARRA LATERAL fixa à esquerda faz toda a navegação. No topo dela há um botão
"Visão geral" e, abaixo, um botão para cada tomada (cada um com o nome e um ponto colorido
de estado). A área de conteúdo à direita troca conforme o botão clicado. No celular, a barra
lateral vira menu recolhível.

- Ao clicar em "Visão geral": mostrar os dados da CASA — consumo total agora em destaque +
  barra de capacidade (consumo atual / P_max, que muda de cor perto do limite) + uma lista
  compacta de todas as tomadas com estado e consumo + indicador de conexão
  (online/sincronizando/offline).
- Ao clicar em uma tomada: mostrar os dados DAQUELA tomada — estado atual (badge colorido),
  consumo agora (W), toggle ligar/desligar manual, selo de "crítica", gráfico de consumo
  (24h/7d), gráfico de barras de probabilidade de uso por hora (24 barras) e uma grade de
  agendamento de 24h clicável.

Estilo: limpo e minimalista, confiável, tema de energia/sustentabilidade; números de consumo
com destaque; cards arredondados; ON=verde, STD=âmbar, OFF=cinza, alerta de teto=laranja/vermelho.
Trate estados de offline/sincronizando, comando manual recusado pelo teto, tomada crítica e
estado vazio. Gere em React + TypeScript + Tailwind.
```

---

*Projeto: TCC — Otimização Energética de Tomadas Inteligentes (ESP32/Rust + Supabase).
Modelo MILP, sensores ZMPT101B/SCT-013, coordenação ESP-NOW. Detalhes técnicos completos em
`specs/001-otimizacao-tomadas-milp/` e `specs/002-refinamento-supabase/`.*
