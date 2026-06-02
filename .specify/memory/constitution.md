<!--
SYNC IMPACT REPORT
==================
Version change: 1.1.0 → 1.2.0
Bump rationale: MINOR. Adicionado o Princípio VIII (Segurança e Isolamento
  Multi-Inquilino), até então AUSENTE, e expandido materialmente o Princípio VI
  (identidade do dispositivo por MAC→UUID e contrato de autenticação por JWT
  por-dispositivo com RLS default-deny). Nenhum princípio foi removido ou
  redefinido de forma incompatível → MINOR (novo princípio + expansão material).

  Decisões que motivaram a emenda (sessão 2026-05-29):
    - Identidade: MAC do ESP32 → `tomada_id` UUID (MAC identifica, não autentica).
    - Auth/Isolamento: JWT por-dispositivo + RLS chaveada em claims (default-deny).
    - Provisionamento: portal AP + Edge Function que registra pelo MAC e emite
      segredo durável + JWT (segredo de assinatura nunca na borda; .bin genérico).

  Histórico:
    1.0.0 (2026-05-28) — Ratificação inicial (7 princípios).
    1.1.0 (2026-05-28) — Princípio VI: Firebase → Supabase.
    1.2.0 (2026-05-29) — +Princípio VIII (Segurança/Isolamento); VI expandido
      (identidade MAC→UUID, auth JWT por-device, RLS default-deny).

Modified principles:
  - VI. "Supabase como Fonte Única de Verdade (Offline-First)" (título mantido)
      corpo expandido: identidade do dispositivo (MAC→UUID) e remissão explícita
      ao Princípio VIII para autenticação/isolamento.

Added sections:
  - VIII. Segurança e Isolamento Multi-Inquilino (Zero-Trust no Edge)

Removed sections: nenhuma

Templates requiring updates:
  - .specify/templates/plan-template.md ...... ✅ compatível (Constitution Check
        é referenciado genericamente; nenhuma edição necessária)
  - .specify/templates/spec-template.md ....... ✅ compatível
  - .specify/templates/tasks-template.md ...... ✅ compatível (já prevê
        "Security hardening" como categoria de tarefa)
  - .specify/templates/checklist-template.md .. ✅ compatível

Follow-up TODOs (artefatos da feature 001 a alinhar em /speckit-plan ou /implement):
  - specs/001-otimizacao-tomadas-milp/contracts/supabase-schema.sql:
      adicionar coluna `mac text unique` em `tomadas`; substituir o RLS
      "políticas a definir" por políticas DEFAULT-DENY chaveadas em
      `auth.jwt()->>'residencia_id'` (e `tomada_id` para escrita do próprio nó);
      remover qualquer suposição de escrita pelo papel `anon`.
  - specs/001-otimizacao-tomadas-milp/data-model.md: incluir `mac` e o segredo
      durável de provisionamento (refresh) no struct Tomada / estado em LittleFS.
  - firmware/src/config_nuvem.rs: remover credencial por-dispositivo do `.env`;
      ler segredo durável + JWT de LittleFS (provisionados em runtime).
  - firmware/src/comunicacao/supabase.rs.template: trocar `anon key` fixa por
      `Authorization: Bearer <JWT por-device>` com renovação; nunca portar o
      segredo de assinatura do JWT para o firmware.
  - firmware/src/comunicacao/wifi_provisioning.rs: coletar claim-code e chamar a
      Edge Function `provisionar`.
  - Supabase (servidor): criar Edge Function de provisionamento/renovação que
      assina o JWT server-side com o segredo do projeto.
  - RISCO TÉCNICO herdado (resolver em /speckit-plan): `minilp` resolve LP
      contínuo; o Princípio I exige integralidade binária (x_{i,s} ∈ {0,1}) via
      branch-and-bound OU backend MILP equivalente.
-->

# Constituição do Sistema de Tomadas Inteligentes Otimizadas (y / TCC)

## Core Principles

### I. Modelagem Matemática Rigorosa e Sempre Factível

A decisão de estado de cada tomada (`OFF`, `STD`, `ON`) DEVE ser produzida pelo
modelo de otimização (MILP), nunca por uma cadeia de `IF/ELSE` no firmware. O
modelo DEVE obedecer à formulação do TCC:

- Variáveis de decisão `x_{i,s}` DEVEM ser estritamente binárias (`{0,1}`); a
  relaxação contínua é proibida como resultado final.
- A restrição R1 (unicidade de estado) e R2 (capacidade global / knapsack `P_max`)
  são INVIOLÁVEIS (hard constraints).
- As restrições de uso ativo (R3) e agendamento (R5) DEVEM ser modeladas como
  *soft constraints* com variáveis de folga (`y_uso`, `y_age`) penalizadas na
  função objetivo. O solver DEVE retornar SEMPRE uma solução factível: a
  infactibilidade nunca pode travar o firmware.

**Rationale**: A crítica central ao modelo anterior foi infactibilidade presa no
firmware e uso indevido de variáveis contínuas. Delegar a violação de regras ao
modelo (via penalidade) elimina deadlocks lógicos e dá rastro auditável.

### II. Complexidade Justificada (Anti-Over-Engineering)

Toda escolha algorítmica e arquitetural DEVE ser justificada por complexidade
real do problema. Especificamente:

- O uso de um solver matemático SÓ se justifica pelo acoplamento global entre
  tomadas via `P_max` (R2). Um cenário de tomada única e desacoplada NÃO DEVE
  invocar branch-and-bound — DEVE recair em decisão direta.
- Qualquer violação desta regra DEVE ser registrada na seção *Complexity
  Tracking* do plano, com a alternativa mais simples e o motivo da rejeição.
- Camadas preditivas (ex.: ML/séries temporais) só entram quando alterarem
  mensuravelmente a decisão ótima.

**Rationale**: O revisor apontou *over-engineering* (problema `O(1)` resolvido com
solver). A complexidade agora é legítima (mochila multi-nó), mas o princípio
guarda contra reincidência.

### III. Confiabilidade do Hardware Embarcado

O firmware DEVE proteger a memória Flash do ESP32 contra desgaste (*flash wear*):

- Dados transitórios (medições horárias, telemetria) DEVEM ser acumulados em RAM
  e consolidados em LittleFS apenas em lote ou em eventos significativos —
  NUNCA gravação contínua hora-a-hora no sistema de arquivos.
- O uso de memória DEVE ser constante/limitado (sem crescimento não-acotado em
  tempo de execução).
- Em falha de energia ou rede, a tomada DEVE recair em um estado local seguro
  pré-definido.

**Rationale**: A gravação horária contínua em SPIFFS/LittleFS degrada a Flash e
compromete confiabilidade em campo — inaceitável num produto de longa vida.

### IV. Inteligência Preditiva Baseada em Histórico

A função objetivo DEVE usar a probabilidade histórica de uso por dispositivo
`Π_{i,t}` (janela variável de `D_i` dias), e não médias móveis cegas. O peso de
criticidade `W_i` DEVE preservar equipamentos essenciais (ex.: roteador) via peso
artificialmente alto `M`. Desligar um aparelho com `Π_{i,t}` alto DEVE ser
penalizado (preservação preditiva de conforto).

**Rationale**: Média móvel de 24h tinha impacto nulo na decisão ótima. `Π_{i,t}`
cruza dia/hora e comportamento individual, evitando desligar algo segundos antes
de o usuário precisar.

### V. Coordenação Multi-Nó Resiliente

Como a restrição `P_max` (R2) correlaciona todas as tomadas, os ESP32 DEVEM
coordenar a decisão global, não decidir isoladamente:

- DEVE existir um protocolo explícito e versionado de comunicação entre nós
  (ESP32 ↔ ESP32) para agregar estado/consumo e distribuir a solução.
- A topologia (coordenador eleito vs. peer-to-peer) DEVE ser documentada no
  plano, com comportamento definido para perda de nó e partição de rede.
- Na ausência de coordenação, cada nó DEVE degradar com segurança para um modo
  local conservador que NÃO viole `P_max` localmente conhecido.

**Rationale**: A otimização global de alocação de recursos é o que eleva o
problema acima de uma árvore de decisão; sem coordenação entre nós, R2 é
inexequível.

### VI. Supabase como Fonte Única de Verdade (Offline-First)

O Supabase é o repositório canônico de configuração, histórico e parâmetros do
modelo (`C_{i,s}`, `Π_{i,t}`, `W_i`, `A_{i,t}`, `P_max`, schedules):

- O esquema do banco DEVE ser modelado e versionado explicitamente antes da
  implementação que o consome.
- Cada dispositivo é identificado pelo seu MAC (mapeado a um `tomada_id` UUID) e
  agrupado por `residencia_id`; a identidade DEVE ser estabelecida no
  provisionamento, nunca presumida pelo cliente.
- O contrato ESP32 ↔ Supabase (autenticação, formato, frequência) DEVE ser
  definido, respeitar o Princípio III (escrita em lote, não contínua) e o
  Princípio VIII (autenticação por-dispositivo e isolamento por residência).
- O sistema DEVE ser *offline-first*: a tomada opera com segurança desconectada
  e reconcilia com o Supabase ao reconectar.
- NÃO DEVE haver fonte de verdade dupla: o Supabase é o único banco em nuvem do
  projeto.

**Rationale**: Centralizar parâmetros e histórico na nuvem habilita a camada
preditiva e o ajuste sem reflash; *offline-first* preserva a confiabilidade
embarcada exigida pelo Princípio III. O Supabase já está parcialmente integrado
(cliente HTTPS de referência no firmware e dashboard), reduzindo o retrabalho.

### VII. Observabilidade e Reprodutibilidade das Decisões

Toda execução do solver DEVE registrar, de forma estruturada: os parâmetros de
entrada, o valor da função objetivo `Z`, os estados escolhidos `x_{i,s}` e
TODA penalidade ativada (`y_uso`, `y_age`) com a regra violada. Uma decisão DEVE
ser reproduzível a partir de suas entradas registradas.

**Rationale**: Soft constraints só são auditáveis se cada violação gerar log
padronizado; reprodutibilidade é pré-requisito para validar o modelo no TCC.

### VIII. Segurança e Isolamento Multi-Inquilino (Zero-Trust no Edge)

O dispositivo de borda é tratado como NÃO-confiável (o firmware pode ser extraído
do chip) e nenhum segredo compartilhado pode conceder acesso aos dados de outra
residência:

- **Identidade ≠ Autenticação**. O MAC do ESP32 IDENTIFICA o dispositivo (coluna
  `mac` única em `tomadas`, mapeada a um `tomada_id` UUID), mas NUNCA o autentica —
  o MAC é difundido em Wi-Fi/ESP-NOW e é público. A autenticação DEVE usar uma
  credencial por-dispositivo, distinta do MAC.
- **Sem segredo compartilhado entre dispositivos**. É PROIBIDO embutir no binário
  uma chave única (ex.: `anon key`) que sirva a todos os nós. Cada dispositivo DEVE
  ter credencial própria, revogável e rotacionável sem reflash.
- **RLS default-deny**. Todas as tabelas DEVEM ter Row Level Security habilitada com
  padrão de NEGAÇÃO; o acesso é concedido apenas por políticas que chaveiam em
  claims do JWT por-dispositivo (ex.: `residencia_id`, `tomada_id`). Um nó só lê e
  escreve a própria residência.
- **Segredo de assinatura nunca na borda**. O segredo que assina os JWT NÃO DEVE,
  em hipótese alguma, residir no firmware. A emissão e renovação de JWT DEVEM
  ocorrer num componente servidor (Edge Function de provisionamento) que valide um
  claim-code e a credencial durável do dispositivo.
- **Menor privilégio**. O `service_role` (ou credencial irrestrita equivalente)
  NUNCA é exposto ao firmware nem ao dashboard cliente; fica restrito ao componente
  servidor.
- **Escritas safety-critical protegidas**. Campos que acionam hardware ou alteram o
  teto elétrico (`rele_aberto`, `p_max`, `estado`, calibração) DEVEM ser graváveis
  apenas pelo dono legítimo (RLS por claim). Comprometer um nó NÃO PODE permitir
  acionar relé ou alterar `P_max` de outra residência.
- **Superfície mínima no binário**. O `.bin` DEVE ser genérico (sem credencial
  por-dispositivo); identidade e segredo durável são provisionados em runtime
  (portal AP → Edge Function → LittleFS).

**Rationale**: O desenho anterior (uma `anon key` única hardcoded + RLS sem
políticas) permitia que qualquer pessoa que extraísse o `.bin` de um único nó
lesse e escrevesse os dados de TODAS as residências — inclusive acionar relés e
alterar `P_max`, o que é falha de *safety*, não só de privacidade. Separar
identidade (MAC) de autenticação (JWT por-device), com RLS default-deny e emissão
server-side, contém o raio de explosão a um único dispositivo e habilita rotação e
revogação sem reflash.

## Restrições Técnicas e Arquitetura

- **Firmware**: Rust sobre `esp-idf-svc` no ESP32; infra portada (LittleFS,
  Wi-Fi STA + AP de provisionamento, SNTP) DEVE ser reutilizada, não reescrita.
- **Solver**: a decisão DEVE garantir integralidade binária (Princípio I). O
  crate `minilp` resolve LP contínuo; portanto é OBRIGATÓRIA uma camada de
  branch-and-bound sobre ele OU um backend MILP equivalente. A localização da
  execução (edge no ESP32, nó coordenador, ou Cloud Function) é decisão de
  `/speckit-plan` e DEVE ponderar os Princípios II e III.
- **Banco de dados**: Supabase (Princípios VI e VIII). O esquema DEVE ser modelado
  nesta iteração com `mac` único em `tomadas` e RLS *default-deny* chaveada em
  claims do JWT por-dispositivo; o cliente `supabase.rs.template` DEVE ser adaptado
  para `Authorization: Bearer <JWT por-device>` com renovação — não a `anon key`
  compartilhada.
- **Provisionamento/identidade**: o `.bin` é genérico; identidade (MAC→UUID),
  segredo durável e JWT são obtidos em runtime via portal AP → Edge Function de
  provisionamento → LittleFS (Princípio VIII). O segredo de assinatura do JWT vive
  apenas no servidor.
- **Dashboard**: Vite + TypeScript; DEVE consumir o Supabase e exibir decisões,
  penalidades e histórico de forma rastreável (Princípio VII).
- **Comunicação entre nós**: protocolo explícito ESP32 ↔ ESP32 (Princípio V).
- **Restrições de build conhecidas** (ESP32/Xtensa) DEVEM ser preservadas:
  `serde_json` em `opt-level = 0`, perfil dev `opt-level = 0`, stack de 40 KB,
  partição LittleFS via `joltwallet/littlefs`.

## Fluxo de Desenvolvimento e Portões de Qualidade

- O trabalho segue o fluxo Spec Kit: `constitution → specify → clarify → plan →
  tasks → implement`, com checklists e análise quando aplicável.
- O *Constitution Check* do `plan-template.md` DEVE ser avaliado antes da Fase 0
  e reavaliado após a Fase 1; violações DEVEM ir para *Complexity Tracking*.
- Toda formulação ou alteração do modelo de otimização DEVE ser validada contra
  a formulação matemática do TCC (conjuntos, parâmetros, variáveis, R1–R5,
  função objetivo) antes do merge.
- Mudanças que afetem escrita em Flash, esquema/auth do Supabase ou o protocolo
  multi-nó DEVEM declarar explicitamente conformidade com os Princípios III,
  {VI + VIII} e V respectivamente. Toda alteração de RLS, credenciais ou
  provisionamento DEVE passar pelo crivo do Princípio VIII (sem segredo
  compartilhado, RLS default-deny, segredo de assinatura fora da borda).

## Governance

Esta constituição prevalece sobre quaisquer outras práticas do projeto. Em caso
de conflito entre um plano/tarefa e um princípio, o princípio vence ou a exceção
DEVE ser justificada e registrada em *Complexity Tracking*.

- **Emendas**: propostas por escrito (PR), com descrição do impacto nos
  artefatos dependentes (templates, specs, planos) e plano de migração quando a
  mudança for incompatível.
- **Versionamento (SemVer)**: MAJOR para remoção/redefinição incompatível de
  princípios ou governança; MINOR para novo princípio/seção ou expansão material;
  PATCH para esclarecimentos e correções não-semânticas.
- **Conformidade**: toda revisão de PR DEVE verificar aderência aos princípios
  aplicáveis. Linguagem vaga ("deveria") é proibida em requisitos normativos —
  usar DEVE/NÃO DEVE com rationale.

**Version**: 1.2.0 | **Ratified**: 2026-05-28 | **Last Amended**: 2026-05-29
