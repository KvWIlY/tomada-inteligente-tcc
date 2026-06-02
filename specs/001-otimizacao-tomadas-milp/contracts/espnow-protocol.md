# Contract — Protocolo ESP-NOW de Coordenação Multi-Nó

Feature: 001-otimizacao-tomadas-milp | Princípio V (coordenação resiliente)

Canal **primário** para honrar o teto global `P_max` (R2) sem depender da nuvem
(offline-first). Cada tomada difunde seu estado/consumo; todos os nós montam o mesmo
`SolverInput` e resolvem o MILP de forma replicada e determinística.

## Transporte

- ESP-NOW broadcast (ou peers registrados, `N ≤ 8`) no mesmo canal Wi-Fi.
- Sem confiabilidade garantida: o protocolo é *stateless* e idempotente (cada mensagem
  carrega um snapshot completo do nó, não deltas).
- Cadência de heartbeat: a cada ciclo de decisão (alinhado a SC-008, ≤10 s).

## Mensagem `HeartbeatTomada` (broadcast periódico)

Payload binário compacto (cabe folgado no limite de ~250 bytes do ESP-NOW). Cada nó
**anuncia os próprios parâmetros do ciclo** (calculados da sua config + sensores), para
que qualquer peer monte o MILP global completo **sem consultar o Supabase** (offline-
first). Os valores de `Π` e `A` são os **da hora corrente** (não os 24 da config).

| Campo | Tipo | Bytes | Descrição |
|-------|------|-------|-----------|
| `versao` | u8 | 1 | versão do protocolo (=3) |
| `tomada_id` | u8 | 1 | índice curto do nó na residência (0..N-1) |
| `epoch` | u32 | 4 | timestamp da leitura (SNTP) |
| `p_medida` | f32 | 4 | potência atual (W) |
| `estado` | u8 | 1 | 0=OFF,1=STD,2=ON |
| `flags` | u8 | 1 | bit0=`U_i`, bit1=`F_i`, bit2=`rele_manual`, bit3=`A_{i,t}` (hora atual) |
| `c_std` | f32 | 4 | `C_{i,STD}` calibrado (W) |
| `c_on` | f32 | 4 | `C_{i,ON}` calibrado (W) |
| `pi_hora` | f32 | 4 | `Π_{i,t}` da hora corrente (0..1) — termo de conforto |
| `w_i` | f32 | 4 | peso de criticidade `W_i` (M se crítico) |
| `hmac` | [u8;16] | 16 | `HMAC-SHA256(residencia_key, bytes[0..28])[..16]` — autentica os 28 bytes de dados |

Total: 44 bytes (28 de dados + 16 de tag). Os 28 bytes de dados são exatamente os campos
`versao..w_i`; o `hmac` cobre todos eles.

## Autenticação e isolamento por residência (v3 — Princípio VIII)

O canal ESP-NOW é público (broadcast, mesmo canal): qualquer rádio próximo recebe e pode
transmitir. Para impedir que um nó malicioso (ou o sistema de um vizinho) injete heartbeats
e manipule o teto `P_max`, o v3 **autentica** cada mensagem:

- **Chave por-residência** (`residencia_key`, 32 bytes): a MESMA para todos os nós de uma
  casa. É cunhada no servidor e entregue ao device **só** na resposta de `/provisionar`
  (ver `provisioning-api.md`), guardada em LittleFS, **nunca embutida no `.bin`** nem lida
  pela Data API (tabela `residencia_secrets` é default-deny). Rotacionável/revogável sem
  reflash (apagar a linha + re-provisionar).
- **Tag HMAC-SHA256** sobre os 28 bytes de dados, truncada em 16 bytes. Na recepção o nó
  recalcula com a própria chave e compara em **tempo ~constante**; se não bate, descarta
  (forjado, corrompido, ou de outra residência cuja chave difere).
- **Anti-replay**: o `epoch` (dentro do HMAC, logo não-forjável) deve estar a no máximo
  **±60 s** do relógio local (nós SNTP-sincronizados). Heartbeats reapresentados expiram.
- **Fail-closed**: sem `residencia_key` válida (ex.: credencial anterior ao v3), o nó
  **desabilita o ESP-NOW** e opera sozinho até re-provisionar — nunca aceita peers não
  autenticados.

> **Modelo de ameaça / limite**: a `residencia_key` é compartilhada na casa. Extrair
> fisicamente UM nó expõe a chave **daquela** residência (permite forjar heartbeats só ali);
> não dá acesso à nuvem (JWT por-device) nem a outra residência. Raio de dano limitado a
> uma casa — coerente com "borda não-confiável" (Princípio VIII). A cifra nativa do ESP-NOW
> (LMK/AES) **não** é usada por ser unicast-only, incompatível com o desenho broadcast.

> `a_it` (R5) viaja como **bit3 das flags**; `pi_hora` e `w_i` como floats. Com isso,
> cada peer tem tudo que o `SolverInput`/`TomadaInput` exige (ver `solver-api.md`) para
> avaliar objetivo, R2, R3, R4 e R5 de **todas** as tomadas.

## Tabela de peers (estado em RAM por nó)

- `peers[id] = { ultima_msg: Heartbeat, recebido_em: epoch }`.
- Peer considerado **vivo** se `recebido_em` dentro de `T_peer` (default: 3× a cadência).
- Peer **ausente** ⇒ degradação segura (ver abaixo).

## Regras de decisão coordenada

1. A cada ciclo, o nó coleta os heartbeats vivos + sua própria leitura.
2. Monta `SolverInput` (uma `TomadaInput` por peer vivo + a si mesmo; ver
   `solver-api.md`). Para os peers, **todos** os campos de `TomadaInput`
   (`c_std`, `c_on`, `u_i`, `f_i`, `pi`, `w_i`, `a_it`) vêm direto do heartbeat; para si
   mesmo, vêm da própria config + sensores.
3. Resolve o MILP localmente; aplica **somente** o estado da sua própria tomada.
4. Como modelo e entradas são idênticos entre nós e o solver é determinístico, todos
   convergem para a mesma alocação global (consistência sem coordenador central).

## Degradação segura (peer ausente — Princípio V)

Um peer sem heartbeat válido entra no problema como **carga fixa** (não como variável
de decisão — assim seus `pi`/`w_i`/`a_it`, que não temos, não são necessários). O
consumo fixo assumido é:
- o **último `p_medida` conhecido**, se existir; senão
- o **pior caso** `C_{i,ON}` (assume ligado no máximo).

Esse consumo fixo é subtraído do `P_max` disponível antes de otimizar os nós vivos.
Isso garante que a soma usada na restrição R2 **nunca subestime** o consumo real ⇒ o nó
nunca decide algo que viole o `P_max` conhecido. Na falta total de peers, o nó decide
apenas para si respeitando sua fatia do teto.

## Requisito de consistência (para `/tasks`)

- Relógio sincronizado via SNTP em todos os nós (mesma `hora` para indexar `Π`/`schedule`).
- `tomada_id` curto (0..N-1) estável por residência (mapeado a partir do `uuid` do
  Supabase no provisionamento).
