# Feature Specification: Refinamento da Implementação do Supabase (Segurança por-Dispositivo e Sincronização Confiável)

**Feature Branch**: `002-refinamento-supabase`

**Created**: 2026-05-30

**Status**: Draft

**Input**: User description: "refinamento — sobre a implementação da parte do Supabase"

## Contexto e Motivação *(por que este refinamento existe)*

A feature `001-otimizacao-tomadas-milp` já definiu o contrato de nuvem desejado
(`contracts/supabase-schema.sql`, `contracts/provisioning-api.md`) com o modelo de
segurança do **Princípio VIII** da constituição: identidade por MAC → `tomada_id`,
autenticação por **JWT por-dispositivo** (role `device`, claims `residencia_id`/
`tomada_id`), **RLS default-deny** e menor privilégio por coluna. A Edge Function de
provisionamento (`supabase/functions/provisionar`) já implementa a emissão/renovação
desse JWT.

Porém, a **implementação real do cliente Supabase no firmware** ainda está no modelo
antigo e em conflito com esse contrato:

- `firmware/src/comunicacao/supabase.rs.template` autentica com uma **anon key
  compartilhada** (`SUPABASE_ANON_KEY`) em `apikey` e `Authorization: Bearer`, e usa
  `TOMADA_ID`/`RESIDENCIA_ID` lidos de `config_nuvem.rs` — ou seja, **credencial
  por-dispositivo embutida no binário**, violando o Princípio VIII.
- O esquema e a RLS ainda **não foram aplicados/validados** contra o cliente real
  (tarefas T012 e T020 da feature 001 pendentes).
- A sincronização hoje é **uma escrita por chamada** (insert unitário de `medicoes`/
  `eventos`), sem envio **em lote** e sem o handshake de confirmação que a política de
  retenção (purga 5 d / cap 7 d) pressupõe.
- A renovação de JWT (em `401` ou perto de `jwt_exp`) e o **pull** de `config`/
  `rele_aberto` da nuvem ainda não estão ligados ao cliente.

Este refinamento **não altera o modelo MILP nem o contrato de nuvem já especificado**;
ele faz a implementação convergir fielmente para esse contrato, eliminando a deriva da
anon key e fechando o ciclo de sincronização confiável (offline-first). O escopo é a
**camada Supabase** (cliente do firmware + aplicação/validação do schema + sincronização
e reconciliação), não os sensores, o solver ou o ESP-NOW.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Autenticação por-dispositivo substituindo a chave compartilhada (Priority: P1)

Cada tomada acessa o Supabase usando **a sua própria credencial provisionada em runtime**
(JWT por-dispositivo obtido via Edge Function), e **nenhum segredo por-dispositivo nem
chave compartilhada vive no binário distribuído**. O binário `.bin` é genérico: a mesma
imagem pode ser gravada em qualquer tomada e, após o provisionamento, cada uma passa a
falar com a nuvem com identidade e permissões próprias.

**Why this priority**: É o item de segurança não-negociável (Princípio VIII) e a causa-
raiz da deriva atual. Sem ele, qualquer dispositivo (ou vazamento do `.bin`) tem acesso
amplo aos dados de todos — o refinamento perde o sentido.

**Independent Test**: Gravar a imagem genérica numa tomada sem credenciais, provisioná-la
pelo portal de pareamento e confirmar que (a) ela passa a ler/escrever na nuvem só com o
JWT obtido, e (b) inspeção do binário e da configuração de bootstrap não revela nenhuma
chave de acesso a dados nem identificadores de outra residência/tomada.

**Acceptance Scenarios**:

1. **Given** uma tomada recém-gravada com a imagem genérica, **When** ela é provisionada,
   **Then** todo acesso subsequente à nuvem usa o JWT por-dispositivo e a configuração de
   bootstrap contém apenas o endereço do serviço e os dados de rede — nenhuma chave de
   acesso a dados nem `tomada_id`/`residencia_id` fixos.
2. **Given** um JWT expirado ou prestes a expirar, **When** a tomada precisa sincronizar,
   **Then** ela renova a credencial automaticamente (com o segredo durável) e retoma a
   sincronização sem intervenção, sem nunca reusar uma chave compartilhada.
3. **Given** uma resposta de acesso negado da nuvem, **When** a tomada a recebe, **Then**
   ela tenta renovar a credencial uma vez e, persistindo a falha, registra o erro e opera
   em modo offline sem travar.

---

### User Story 2 - Isolamento multi-inquilino verificável (Priority: P1)

Os dados de cada residência e de cada tomada ficam **isolados por padrão**: uma tomada só
lê/escreve o que o contrato de segurança permite (a própria linha para escrita; as
tomadas da mesma residência para leitura de coordenação), nunca dados de outra residência.
Colunas sensíveis (comando manual do relé, teto elétrico, identidade de hardware) **não
podem** ser escritas pelo dispositivo — só pelo dono humano via dashboard. O acesso
anônimo é negado.

**Why this priority**: O isolamento é o que torna o sistema seguro para múltiplos usuários
reais; sem validação ponta-a-ponta, a RLS pode estar configurada mas não exercida pelo
cliente real, deixando uma falsa sensação de segurança.

**Independent Test**: Com duas residências provisionadas, usar a credencial de uma e
tentar ler/escrever dados da outra (deve falhar); tentar escrever colunas reservadas com
a credencial de dispositivo (deve falhar); tentar qualquer acesso sem credencial (deve
falhar); confirmar que as leituras/escritas legítimas da própria tomada funcionam.

**Acceptance Scenarios**:

1. **Given** a credencial da residência A, **When** ela tenta ler ou escrever dados de uma
   tomada da residência B, **Then** o acesso é negado.
2. **Given** a credencial de uma tomada, **When** ela tenta escrever uma coluna reservada
   ao dono (comando manual do relé, teto elétrico, identidade de hardware, vínculo de
   residência), **Then** a escrita é negada, mas as colunas permitidas (estado decidido,
   calibração, telemetria, configuração versionada) são aceitas.
3. **Given** nenhuma credencial válida, **When** uma requisição é feita às tabelas de
   dados, **Then** ela é negada por padrão.
4. **Given** a credencial da própria tomada, **When** ela lê as tomadas-irmãs da mesma
   residência (para coordenação/fallback) e escreve a própria linha, **Then** ambas as
   operações são permitidas.

---

### User Story 3 - Sincronização em lote e reconciliação offline-first (Priority: P2)

As medições (1/min) e os eventos são enviados à nuvem **em lote**, e cada registro só é
considerado **confirmado** localmente após a nuvem aceitá-lo. Quando não há conectividade,
a tomada continua medindo e acumulando localmente e **reconcilia ao reconectar**, sem
duplicar registros já aceitos e sem perder dados ainda não confirmados (dentro da janela
de retenção). A rotina de retenção local só descarta dados **já confirmados** na nuvem,
respeitando o limite rígido de 7 dias.

**Why this priority**: Fecha o ciclo de confiabilidade pressuposto pela política de
retenção (purga 5 d / cap 7 d) e pela proteção de flash (Princípio III). Sem o handshake
de confirmação e o envio em lote, a purga pode apagar dados não persistidos ou a flash
sofre escrita excessiva.

**Independent Test**: Gerar medições com a nuvem indisponível por um período; restaurar a
conectividade e confirmar que o lote acumulado é enviado, marcado como confirmado e que a
purga subsequente remove apenas os confirmados; reenviar não cria duplicatas.

**Acceptance Scenarios**:

1. **Given** várias medições acumuladas localmente, **When** há conectividade, **Then**
   elas são enviadas em lote e marcadas como confirmadas após o aceite da nuvem.
2. **Given** a nuvem indisponível, **When** a tomada gera medições, **Then** elas são
   bufferizadas/persistidas localmente sem perda e enviadas ao reconectar.
3. **Given** um reenvio do mesmo conjunto (ex.: confirmação perdida), **When** o lote é
   reenviado, **Then** a nuvem não acumula registros duplicados.
4. **Given** dados confirmados fora da janela de retenção, **When** a purga executa,
   **Then** apenas os confirmados são removidos; dados não confirmados são preservados até
   a confirmação ou até o limite rígido de 7 dias.

---

### User Story 4 - Pull de configuração e comando manual a partir da nuvem (Priority: P3)

A tomada obtém da nuvem a **configuração mais recente** (parâmetros do modelo, agendamento,
criticidade) quando há uma versão mais nova que a local, e respeita o **comando manual de
ligar/desligar** definido pelo dono no dashboard — sempre subordinado ao teto elétrico
global, que permanece inviolável.

**Why this priority**: Torna o dashboard a fonte de verdade da configuração e do comando
manual (FR-010a da 001), fechando o fluxo bidirecional nuvem↔dispositivo. É P3 porque o
sistema decide localmente mesmo sem o pull; o pull melhora a experiência de controle.

**Independent Test**: Alterar a configuração e o comando manual no dashboard, aguardar o
ciclo de pull e confirmar que a tomada adota a nova configuração (por versão) e aplica o
comando manual, exceto quando ele violaria o teto elétrico.

**Acceptance Scenarios**:

1. **Given** uma configuração mais nova na nuvem (versão maior), **When** a tomada faz o
   pull, **Then** ela adota a nova configuração e atualiza sua versão local.
2. **Given** uma configuração na nuvem com versão igual ou menor, **When** a tomada faz o
   pull, **Then** ela mantém a configuração local (sem regressão).
3. **Given** um comando manual de relé no dashboard, **When** a tomada o lê, **Then** ela o
   aplica, salvo se isso violar o teto elétrico global, caso em que a segurança elétrica
   prevalece.

---

### Edge Cases

- **Provisionamento incompleto/perdido**: tomada sem credencial válida não acessa a nuvem;
  permanece em modo de pareamento/offline sem travar nem tentar uma chave compartilhada.
- **Relógio fora de sincronia**: um JWT considerado expirado por desvio de relógio dispara
  renovação; falha persistente é registrada e a operação segue offline.
- **Schema aplicado parcialmente**: se uma tabela/política estiver ausente, a operação
  correspondente falha de forma observável (registro de evento), sem corromper o estado
  local nem expor dados.
- **Confirmação perdida após escrita aceita**: o reenvio não pode duplicar; a identidade
  natural do registro (tomada + instante) garante idempotência.
- **Credencial revogada**: renovação passa a falhar; a tomada para de escrever na nuvem,
  registra o evento e continua decidindo localmente.
- **Resposta da nuvem maior que o esperado** (lista de tomadas-irmãs): o cliente processa
  sem estourar memória, degradando para os dados disponíveis.

## Requirements *(mandatory)*

### Functional Requirements

**Autenticação e identidade (Princípio VIII)**

- **FR-001**: O cliente de nuvem do firmware MUST autenticar exclusivamente com o **JWT
  por-dispositivo** provisionado em runtime, enviado como credencial de portador; MUST NOT
  usar uma chave de acesso compartilhada (anon key) para acessar dados.
- **FR-002**: O binário distribuído MUST ser genérico: nenhuma credencial por-dispositivo
  (segredo durável, JWT, `tomada_id`, `residencia_id`) e nenhuma chave de acesso a dados
  MUST estar embutida em tempo de compilação. A configuração de bootstrap MUST conter
  apenas o endereço do serviço de nuvem e os dados de rede necessários ao primeiro acesso.
- **FR-003**: O cliente MUST renovar o JWT automaticamente quando ele estiver perto de
  expirar ou quando a nuvem responder com acesso negado, usando o segredo durável
  provisionado; após falha de renovação, MUST registrar o erro e degradar para operação
  offline sem travar.
- **FR-004**: As credenciais provisionadas (segredo durável, identificadores, cache do
  JWT) MUST ser lidas do armazenamento local seguro do dispositivo, não de constantes de
  compilação.

**Schema, RLS e isolamento**

- **FR-005**: O esquema de dados do contrato (tabelas, role de dispositivo, concessões por
  coluna e políticas de RLS default-deny) MUST ser aplicado à nuvem de forma idempotente e
  verificável.
- **FR-006**: Uma tomada MUST ser capaz de escrever **apenas a própria linha** de
  configuração/telemetria e MUST NOT escrever colunas reservadas ao dono (comando manual
  do relé, teto elétrico, identidade de hardware, vínculo de residência).
- **FR-007**: Uma tomada MUST poder ler as tomadas da **própria residência** (para
  coordenação/fallback) e MUST NOT ler nem escrever dados de outra residência.
- **FR-008**: O acesso anônimo (sem credencial válida) às tabelas de dados MUST ser negado
  por padrão.
- **FR-009**: O isolamento MUST ser validado ponta-a-ponta contra o cliente real (não só
  na configuração do banco): tentativas cruzadas entre residências e escritas em colunas
  reservadas MUST falhar; operações legítimas MUST funcionar.

**Sincronização, reconciliação e retenção**

- **FR-010**: Medições e eventos MUST ser enviados à nuvem **em lote**, não uma escrita por
  registro, respeitando a proteção de flash (sem gravação contínua) do Princípio III.
- **FR-011**: Cada registro local MUST ter um indicador de **confirmação** que só é
  marcado após a nuvem aceitar o registro.
- **FR-012**: Quando offline, a tomada MUST acumular medições/eventos localmente sem perda
  e MUST reconciliá-los ao reconectar.
- **FR-013**: O reenvio de registros (após confirmação perdida) MUST ser idempotente — a
  nuvem MUST NOT acumular duplicatas para o mesmo registro (identidade natural: tomada +
  instante).
- **FR-014**: A rotina de retenção MUST remover localmente apenas os registros
  **confirmados** na nuvem, preservando os não confirmados até a confirmação; entretanto,
  qualquer registro que exceda **7 dias** MUST ser descartado mesmo sem confirmação, para
  proteger a memória do dispositivo.

**Pull de configuração e comando**

- **FR-015**: A tomada MUST adotar a configuração da nuvem somente quando a **versão**
  remota for maior que a local, atualizando sua versão local; versões iguais ou menores
  MUST NOT regredir a configuração local.
- **FR-016**: A tomada MUST ler e aplicar o **comando manual** de relé definido pelo dono,
  exceto quando isso violar o teto elétrico global, que permanece inviolável.

**Observabilidade**

- **FR-017**: Falhas relevantes de sincronização (acesso negado persistente, credencial
  revogada, schema ausente, falha de renovação) MUST ser registradas como eventos
  observáveis, sem expor segredos.

### Key Entities *(include if feature involves data)*

- **Credencial Local do Dispositivo**: segredo durável, identificadores de tomada e
  residência, e cache do JWT (token de regime + expiração); provisionada em runtime,
  persistida localmente, nunca embutida no binário.
- **Registro Sincronizável (Medição/Evento)**: dado de telemetria/observabilidade com
  identidade natural (tomada + instante) e indicador de confirmação na nuvem; base para a
  reconciliação idempotente e a retenção.
- **Configuração Versionada da Tomada**: parâmetros do modelo + agendamento + criticidade,
  com um número de versão que governa o pull (adoção só de versão mais nova).
- **Comando Manual**: intenção do dono (relé aberto/fechado) lida da nuvem e aplicada
  subordinada ao teto elétrico.
- **Política de Acesso (RLS)**: regras que vinculam cada operação aos claims do JWT
  (residência/tomada) e às colunas permitidas por papel.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% dos acessos do firmware à nuvem usam o JWT por-dispositivo; 0 acessos
  usam chave compartilhada.
- **SC-002**: Inspeção do binário genérico e da configuração de bootstrap revela 0
  credenciais de acesso a dados e 0 identificadores fixos por-dispositivo.
- **SC-003**: Em 100% das tentativas de acesso cruzado entre residências, o acesso é
  negado; em 100% das tentativas de escrita em colunas reservadas pelo dispositivo, a
  escrita é negada.
- **SC-004**: 100% das requisições sem credencial válida às tabelas de dados são negadas.
- **SC-005**: Após uma janela offline, 100% das medições acumuladas (dentro da retenção)
  são sincronizadas ao reconectar, sem duplicatas.
- **SC-006**: A purga local nunca remove um registro não confirmado dentro da janela de
  7 dias; nenhum dado já confirmado na nuvem é perdido pela purga.
- **SC-007**: Nenhum dado permanece localmente por mais de 7 dias.
- **SC-008**: A sincronização ocorre **em lote** — não há gravação na flash por minuto nem
  uma chamada de rede por registro.
- **SC-009**: 100% das configurações remotas com versão maior são adotadas; 0 regressões a
  partir de versões iguais/menores.
- **SC-010**: 100% das falhas de sincronização relevantes geram um evento observável sem
  vazar segredos.

## Assumptions

- O contrato de nuvem da feature 001 (`contracts/supabase-schema.sql` e
  `contracts/provisioning-api.md`) é a referência **autoritativa**; este refinamento faz a
  implementação convergir para ele, sem redefinir o modelo de segurança nem o MILP.
- A Edge Function de provisionamento (`supabase/functions/provisionar`) já emite e renova o
  JWT por-dispositivo e permanece como o único ponto que assina o token; este refinamento
  não a reescreve, apenas a consome corretamente no firmware.
- O provisionamento (pareamento via portal e persistência das credenciais) é tratado pela
  fase de provisionamento já planejada na 001; aqui assume-se que a credencial local está
  disponível para o cliente.
- A identidade natural de uma medição é o par (tomada, instante), suficiente para
  idempotência do reenvio.
- A plataforma, os sensores, o solver e a coordenação ESP-NOW permanecem como na 001 e
  estão **fora do escopo** deste refinamento.
- Retenção local de no máximo 7 dias, com purga periódica de dados já confirmados,
  conforme a 001.

### Out of Scope

- Alterações no modelo MILP (tópicos 1–4) ou na função objetivo.
- Reescrita da Edge Function de provisionamento ou do fluxo de pareamento.
- Sensores (ZMPT101B/SCT-013), atuação do relé e cálculo de potência.
- Coordenação multi-nó via ESP-NOW.
- Migração de infraestrutura para qualquer outro provedor (o banco é Supabase).
- Funcionalidades novas de dashboard além do consumo já previsto na 001.
