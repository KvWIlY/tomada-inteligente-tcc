# Feature Specification: Otimização Energética de Tomadas Inteligentes (Modelo MILP Multi-Nó)

**Feature Branch**: `001-otimizacao-tomadas-milp`

**Created**: 2026-05-28

**Status**: Draft

**Input**: User description: "implementar do tópico 1 ao 4 do review_modelo.pdf sem alterações; sistema em Rust rodando no ESP32; sensores ZMPT101B (tensão) e SCT-013 20A/1V (corrente); comunicação entre ESP32 via ESP-NOW; banco em Supabase (a modelar); dados ficam no ESP32 por 1 semana e são limpos após estarem na nuvem, ou a cada 5 dias"

## Clarifications

### Session 2026-05-28

- Q: Como o hardware (relé binário) realiza os 3 estados OFF/STD/ON? → A: Opção A —
  relé só abre/fecha. `OFF` = relé aberto (corta energia); `STD` e `ON` = relé fechado
  (energizado). A diferença entre `STD` e `ON` é inferida pelos sensores (consumo
  fantasma vs. uso ativo), não atuada. Confirmado por projeto de referência `tcc/`
  (`tomada-solver/src/tipos.rs`, `firmware/src/decisao/atuador.rs`, `solver.rs`).
- Q: Qual a granularidade de tempo `t` e como `Π_{i,t}` é calculado? → A: Opção A —
  faixas de 1 hora (24 por dia). `Π_{i,t}` é a média de uso sobre a janela de `D_i`
  dias para aquela faixa horária (ex.: fração dos últimos `D_i` dias em que o aparelho
  esteve em uso naquela hora).
- Q: O usuário pode sobrepor a decisão do otimizador? → A: (1) A única configuração que
  afeta o modelo é marcar uma tomada como "não pode desligar" (geladeira, roteador) —
  corresponde a `is_critical`/`W_i = M`. (2) Pelo dashboard o usuário também pode ligar
  e desligar manualmente uma tomada. O teto global `P_max` (R2) permanece inviolável.
- Q: Quantas tomadas (`N`) o sistema deve suportar? → A: Opção A — até ~8 tomadas
  (protótipo residencial), dentro do limite de peers do ESP-NOW.
- Q: Cadência de amostragem e persistência das medições? → A: Opção B — persistir uma
  medição agregada **a cada 1 minuto** (resolução fina). Amostragem dos sensores
  continua rápida em RAM (RMS) para `U_i`/`F_i` e atuação. Salvaguarda (Princípio III):
  os registros de 1 min são bufferizados em RAM e gravados na flash **em lote**, e a
  sincronização com o Supabase também é em lote — sem gravação na flash a cada minuto.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Decisão global de estado sob teto elétrico (Priority: P1)

Em uma residência com várias tomadas inteligentes, o sistema decide automaticamente
o estado de cada tomada (`OFF`, `STD`, `ON`) de modo a minimizar o consumo total de
energia sem que a soma das potências ultrapasse a capacidade elétrica disponível
(`P_max`). A decisão considera todas as tomadas simultaneamente (alocação global de
recursos), priorizando desligar os aparelhos de menor relevância.

**Why this priority**: É o núcleo do TCC — o problema de otimização combinatorial
(mochila/knapsack) que justifica o uso de um solver. Sem ele, não há produto.

**Independent Test**: Fornecer um conjunto de tomadas com potências por estado e um
`P_max`, e verificar que o sistema retorna exatamente um estado por tomada, que a
soma das potências respeita `P_max`, e que o consumo total é o mínimo possível.

**Acceptance Scenarios**:

1. **Given** N tomadas cuja soma das potências em `ON` excede `P_max`, **When** o
   sistema decide, **Then** ele desliga/reduz tomadas suficientes para respeitar
   `P_max`, priorizando as de menor probabilidade histórica de uso.
2. **Given** que todas as tomadas cabem dentro de `P_max`, **When** o sistema decide,
   **Then** nenhuma tomada é desligada apenas por restrição de capacidade.
3. **Given** qualquer entrada válida, **When** o sistema decide, **Then** cada tomada
   recebe exatamente um estado (unicidade de estado).

---

### User Story 2 - Conforto preditivo e regras (soft/hard constraints) (Priority: P2)

O sistema preserva o conforto do usuário e respeita regras operacionais: não desliga
indevidamente aparelhos em uso ativo, respeita agendamentos, elimina consumo fantasma
e evita desligar aparelhos com alta probabilidade de uso naquele horário. Conflitos
entre regras (ex.: horário proibido mas aparelho em uso real) são resolvidos pelo
modelo via penalidades, nunca travando o sistema.

**Why this priority**: Transforma a decisão de "apenas economizar" em uma decisão
equilibrada e sempre factível, eliminando a infactibilidade que travava o firmware
anterior.

**Independent Test**: Forçar um conflito (uso ativo detectado + horário não permitido)
e verificar que o sistema retorna uma solução válida com a penalidade registrada,
sem erro de infactibilidade.

**Acceptance Scenarios**:

1. **Given** uso ativo detectado em uma tomada, **When** o sistema decide, **Then** o
   estado OFF só é escolhido se a penalidade de uso for contabilizada.
2. **Given** consumo fantasma detectado, **When** o sistema decide, **Then** o estado
   STD é proibido para aquela tomada.
3. **Given** um aparelho fora do horário agendado, **When** o sistema decide, **Then**
   ligar (ON) só ocorre absorvendo a penalidade de agendamento.
4. **Given** alta probabilidade histórica de uso em um horário, **When** o sistema
   decide, **Then** desligar aquele aparelho recebe penalização de conforto.
5. **Given** um aparelho marcado como crítico, **When** o sistema decide, **Then** ele
   é preservado (peso de criticidade alto).

---

### User Story 3 - Coordenação multi-nó entre tomadas (Priority: P2)

As tomadas inteligentes trocam entre si as informações de estado e consumo necessárias
para que a restrição global de capacidade (`P_max`) seja avaliada de forma coordenada,
em vez de cada tomada decidir isoladamente. O sistema permanece seguro quando um nó
fica indisponível.

**Why this priority**: A restrição `P_max` correlaciona todas as tomadas; sem
coordenação entre nós, a otimização global é inexequível.

**Independent Test**: Simular múltiplos nós reportando consumo e verificar que a
decisão global respeita `P_max`; remover um nó e verificar que o sistema degrada para
um modo seguro sem violar o teto conhecido.

**Acceptance Scenarios**:

1. **Given** múltiplas tomadas ativas, **When** uma decisão global é calculada,
   **Then** ela usa o consumo agregado de todos os nós alcançáveis.
2. **Given** que um nó deixa de responder, **When** o sistema decide, **Then** ele
   degrada para um modo local conservador que não viola o `P_max` conhecido.

---

### User Story 4 - Medição, persistência na nuvem e retenção local (Priority: P3)

Cada tomada mede tensão e corrente, calcula a potência consumida e registra essas
medições. Os dados e decisões são enviados para a nuvem; localmente, a tomada mantém
um histórico recente e limpa os dados antigos para proteger a memória do dispositivo.

**Why this priority**: Habilita o histórico (probabilidade de uso) e a observabilidade,
além de garantir confiabilidade do hardware a longo prazo.

**Independent Test**: Gerar medições por vários dias, confirmar persistência na nuvem
e verificar que os dados locais são limpos conforme a política de retenção sem perda
dos dados já confirmados na nuvem.

**Acceptance Scenarios**:

1. **Given** leituras de tensão e corrente, **When** uma medição é feita, **Then** a
   potência é calculada e registrada com data/hora.
2. **Given** medições acumuladas, **When** há conectividade, **Then** os dados são
   persistidos na nuvem.
3. **Given** dados já confirmados na nuvem e fora da janela de retenção, **When** a
   rotina de limpeza executa, **Then** esses dados são removidos do dispositivo.

---

### Edge Cases

- **Sem conectividade com a nuvem**: a tomada continua medindo e decidindo localmente;
  sincroniza ao reconectar; dados locais não são perdidos antes da confirmação (dentro
  da janela de retenção).
- **Conflito de regras**: uso ativo real em horário proibido → resolvido por variáveis
  de folga com penalidade, nunca por erro.
- **Demanda acima do teto**: se nem desligando tudo o conforto puder ser mantido, o
  modelo prioriza por probabilidade histórica e criticidade e registra penalidades.
- **Perda de nó / partição de rede ESP-NOW**: degradação para modo local seguro.
- **Memória local cheia antes da sincronização**: comportamento definido para proteger
  a flash sem corromper dados.
- **Falha de sensor** (tensão ou corrente): medição inválida é sinalizada e não
  contamina o histórico de probabilidade.

## Requirements *(mandatory)*

### Functional Requirements

**Modelo de decisão (tópicos 1–4 do documento, sem alterações)**

- **FR-001**: O sistema MUST decidir, para cada tomada, exatamente um estado entre
  `{OFF, STD, ON}` (unicidade de estado — R1).
- **FR-002**: O sistema MUST garantir que a soma das potências dos estados escolhidos
  não exceda `P_max` (restrição global de capacidade — R2, inviolável).
- **FR-003**: O sistema MUST minimizar a função objetivo definida no tópico 4 do
  documento (consumo direto + penalização de conforto preditivo + penalidades de
  folga), sem alterar a formulação.
- **FR-004**: As variáveis de decisão de estado MUST ser binárias; resultados
  fracionários são inválidos como decisão final.
- **FR-005**: O sistema MUST sempre retornar uma solução factível, usando variáveis de
  folga para uso ativo e agendamento; infactibilidade NÃO MUST travar o sistema.
- **FR-006**: Quando uso ativo for detectado em uma tomada, o estado OFF MUST exigir o
  pagamento da penalidade de uso na função objetivo (R3, soft).
- **FR-007**: Quando consumo fantasma for detectado em uma tomada, o estado STD MUST
  ser proibido (R4, inviolável).
- **FR-008**: Quando o horário não for permitido para uma tomada, o estado ON MUST
  exigir o pagamento da penalidade de agendamento (R5, soft).
- **FR-009**: Desligar uma tomada com alta probabilidade histórica de uso no instante
  atual MUST ser penalizado (preservação de conforto preditivo).
- **FR-010**: Equipamentos marcados pelo usuário como "não pode desligar" (ex.:
  geladeira, roteador) MUST ser tratados como críticos (`is_critical`), recebendo peso
  de criticidade artificialmente alto (`W_i = M`) que os preserva na decisão. Esta é a
  única configuração do usuário que altera o modelo de otimização.
- **FR-010a**: O usuário MUST poder ligar e desligar manualmente uma tomada pelo
  dashboard. O comando manual é respeitado pelo sistema, porém o teto global `P_max`
  (R2) permanece inviolável — um comando que violaria `P_max` NÃO MUST ser aplicado em
  detrimento da segurança elétrica.

**Parâmetros e entradas do modelo**

- **FR-011**: O sistema MUST derivar o indicador de uso ativo comparando a potência
  atual de cada tomada com um limiar configurável por tomada.
- **FR-012**: O sistema MUST derivar o indicador de consumo fantasma a partir do tempo
  ocioso de cada tomada em relação a um limite configurável por tomada.
- **FR-013**: O sistema MUST manter, por tomada, a probabilidade histórica de uso
  `Π_{i,t}` indexada por **faixa horária do dia (24 faixas)**, calculada como a média
  de uso naquela faixa sobre uma janela configurável de `D_i` dias (fração dos últimos
  `D_i` dias em que a tomada esteve em uso na hora correspondente).
- **FR-014**: O sistema MUST permitir a definição de um agendamento (horários
  permitidos) por tomada.
- **FR-015**: O sistema MUST permitir configurar potência por estado, capacidade
  `P_max`, pesos da função objetivo, peso de criticidade e limiares.

**Atuação física**

- **FR-016a**: A atuação MUST ser binária via relé único: `OFF` abre o relé (corta
  energia); `STD` e `ON` mantêm o relé fechado (energizado). A distinção entre `STD` e
  `ON` MUST ser inferida pelos sensores (consumo fantasma vs. uso ativo), não atuada.

**Medição**

- **FR-016**: Cada tomada MUST amostrar tensão e corrente em alta frequência (em RAM)
  para calcular a potência (RMS), derivar `U_i`/`F_i` e atuar; e MUST persistir uma
  medição agregada **a cada 1 minuto**.
- **FR-016b**: Os registros de 1 minuto MUST ser bufferizados em RAM e gravados na
  memória não volátil **em lote**, e sincronizados com o Supabase **em lote** — nunca
  uma escrita em flash por minuto (Princípio III).
- **FR-017**: Medições inválidas (falha de sensor) MUST ser sinalizadas e excluídas do
  cálculo de probabilidade histórica.

**Coordenação multi-nó**

- **FR-018**: As tomadas MUST trocar entre si as informações de estado/consumo
  necessárias para avaliar a restrição global `P_max`.
- **FR-019**: O sistema MUST definir um comportamento seguro de degradação quando um nó
  ficar indisponível, sem violar o `P_max` conhecido localmente.

**Persistência, nuvem e retenção**

- **FR-020**: O sistema MUST persistir medições e decisões na nuvem, segundo um esquema
  de dados modelado explicitamente.
- **FR-021**: Cada tomada MUST acumular dados transitórios em memória volátil e
  consolidá-los em armazenamento não volátil em lote (proteção contra desgaste da
  memória flash), sem gravação contínua a cada leitura.
- **FR-022**: Os dados locais MUST ser retidos por no máximo 7 dias.
- **FR-023**: O sistema MUST executar uma rotina de purga a cada 5 dias que remove do
  dispositivo apenas os dados já confirmados como persistidos na nuvem. Dados ainda não
  confirmados MUST ser preservados até a confirmação; entretanto, qualquer dado que
  exceda 7 dias MUST ser descartado mesmo sem confirmação, para proteger a memória
  flash (Princípio III). A perda do dado mais antigo é aceita no caso de uma
  indisponibilidade da nuvem superior a 7 dias.
- **FR-024**: Toda decisão de otimização MUST ser registrada com suas entradas, o valor
  da função objetivo, os estados escolhidos e todas as penalidades ativadas, de modo
  que a decisão seja reproduzível.

### Key Entities *(include if feature involves data)*

- **Tomada Inteligente (Nó)**: dispositivo controlável; identidade, estado atual,
  se é crítico, peso de criticidade, limiar de uso ativo, limite de ociosidade, janela
  de histórico em dias.
- **Medição**: tensão, corrente, potência calculada, data/hora, tomada de origem,
  indicador de validade.
- **Estado Operacional**: um de `OFF`, `STD`, `ON`.
- **Parâmetros do Modelo**: potência por tomada e estado, capacidade global `P_max`,
  pesos da função objetivo, peso de criticidade.
- **Histórico / Probabilidade de Uso**: probabilidade de uso por tomada e instante,
  derivada do histórico de consumo.
- **Agendamento**: horários permitidos por tomada e instante.
- **Decisão de Otimização**: estados escolhidos por tomada, valor da função objetivo,
  penalidades de uso e de agendamento ativadas, data/hora.
- **Registro de Penalidade/Evento**: regra violada, tomada afetada, magnitude da
  penalidade, data/hora.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% das execuções de decisão retornam uma solução factível (nenhum
  travamento por infactibilidade).
- **SC-002**: Em 100% das decisões emitidas, a soma das potências não excede `P_max`.
- **SC-003**: Em 100% das decisões, cada tomada recebe exatamente um estado.
- **SC-004**: 100% das violações de regra de uso ativo ou agendamento geram um registro
  de penalidade rastreável.
- **SC-005**: Nenhum dado permanece localmente no dispositivo por mais de 7 dias.
- **SC-006**: Em caso de perda de um nó, o sistema continua emitindo decisões seguras
  sem exceder o teto conhecido em 100% dos casos testados.
- **SC-007**: Qualquer decisão registrada pode ser reproduzida a partir de suas
  entradas, gerando resultado idêntico.
- **SC-008**: A decisão de estado é atualizada em até 10 segundos após uma nova leitura
  de medição.
- **SC-009**: Nenhum dado já confirmado na nuvem é perdido pela rotina de limpeza local.

## Assumptions

- **Banco de dados em nuvem**: Supabase (esquema a ser modelado nesta feature). Isto
  **substitui** a menção a Firebase no Princípio VI da constituição — emenda pendente
  (ver nota ao final do fluxo).
- **Plataforma**: sistema implementado em Rust, executando em ESP32, reutilizando a
  infra já portada (LittleFS, Wi-Fi STA + AP de provisionamento, SNTP).
- **Sensores**: tensão via ZMPT101B; corrente via SCT-013 20A/1V; a potência é obtida
  a partir da tensão e da corrente medidas.
- **Comunicação entre tomadas**: ESP-NOW.
- **Escala**: o sistema deve suportar até ~8 tomadas (`N ≤ 8`) num protótipo
  residencial, dentro do limite de peers do ESP-NOW.
- **Modelo matemático**: tópicos 1 a 4 do `review_modelo.pdf` implementados sem
  alterações (conjuntos, parâmetros, variáveis de decisão e formulação com R1–R5).
- **Pesos**: as penalidades de folga dominam a função objetivo (β, γ ≫ 1) e a
  criticidade usa peso alto (M ≫ 1).
- **Retenção**: dados locais mantidos por no máximo 7 dias, com purga periódica
  (candidata: a cada 5 dias) de dados já confirmados na nuvem — cadência exata sujeita
  à clarificação FR-023.
- **Local de execução do solver** (no próprio ESP32 / em um nó coordenador / na nuvem)
  é uma decisão de planejamento (`/speckit-plan`), fora do escopo desta especificação.

### Out of Scope

- Predição avançada por aprendizado de máquina (ARIMA/LSTM) — o histórico de uso atual
  usa a probabilidade definida no documento, não modelos preditivos adicionais.
- Migração de qualquer infraestrutura para Firebase (o banco é Supabase).
