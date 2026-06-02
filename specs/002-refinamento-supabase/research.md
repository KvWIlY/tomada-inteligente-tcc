# Phase 0 — Research & Decisões Técnicas

Feature: Refinamento da Implementação do Supabase (Segurança por-Dispositivo e Sincronização Confiável)
Date: 2026-05-30 | Branch: `002-refinamento-supabase`

Resolve as incógnitas do Technical Context. Cada decisão lista alternativas e a
justificativa, ancorada na Constituição (v1.2.0) e nos contratos autoritativos da feature
001 (`supabase-schema.sql`, `provisioning-api.md`). Não há `NEEDS CLARIFICATION` em aberto
— o contrato de nuvem já está fixado; este refinamento decide **como consumi-lo
fielmente**.

---

## 1. Cabeçalho de autenticação: JWT por-dispositivo, sem `apikey`

**Decisão**: Toda requisição ao PostgREST envia **apenas** `Authorization: Bearer <jwt>`,
com o JWT por-dispositivo do cache de credenciais. **Remover** o header `apikey` e a
constante `SUPABASE_ANON_KEY` do firmware.

**Contexto**: O `supabase.rs.template` atual envia `apikey: ANON_KEY` +
`Authorization: Bearer ANON_KEY`. O JWT por-device carrega `role=device` e os claims que a
RLS usa; o PostgREST faz `SET ROLE device` a partir dele. A anon key é uma credencial
compartilhada — proibida no binário (Princípio VIII).

**Alternativas consideradas**:
- Manter `apikey` com a anon key “só para passar pelo gateway”: rejeitado — reintroduz a
  credencial compartilhada no `.bin` e mascara a auth real.
- Usar a `publishable key` nova do Supabase: ainda é chave compartilhada por projeto;
  não dá isolamento por inquilino. Rejeitado.

**Implicações**: Em `401`/`403`, o cliente chama a renovação (decisão 2) uma vez e repete;
persistindo, registra evento e segue offline. O PostgREST aceita o token sem `apikey`
quando ele é assinado com o JWT secret do projeto (já garantido pela Edge Function).

---

## 2. Renovação do JWT e disparo por expiração/`401`

**Decisão**: `provisionamento::obter_jwt_valido()` retorna o JWT do cache se faltar mais
que uma **margem** (ex.: 5 min) para `jwt_exp`; caso contrário chama
`renovar(tomada_id, device_secret)` e regrava o cache. Em resposta `401`/`403` de qualquer
operação, força uma renovação e **um** retry; nova falha → evento + modo offline.

**Contexto**: O JWT tem TTL curto (default 1 h na Edge Function). O `device_secret` é o
segredo durável persistido; o JWT vive em RAM/cache curto.

**Alternativas consideradas**:
- Renovar a cada requisição: desperdício de rede/bateria e mais escrita de cache.
  Rejeitado.
- Renovar só reativamente no `401`: simples, mas adiciona latência ao primeiro flush após
  expirar. Mitigado pela renovação proativa por margem (híbrido adotado).

**Implicações**: Desvio de relógio (SNTP) pode marcar o JWT como expirado cedo — aceitável,
apenas dispara renovação. O cache do JWT em LittleFS é gravado **em lote** com o resto
(não a cada renovação isolada se possível), respeitando o Princípio III.

---

## 3. Idempotência da sincronização (reenvio sem duplicar)

**Decisão**: Usar a **identidade natural** `(tomada_id, timestamp)` de `medicoes` (já
`unique` no schema da 001) e enviar o lote com `Prefer: resolution=ignore-duplicates`
(upsert que ignora conflitos). Eventos usam `(tomada_id, timestamp, tipo)` — quando houver
risco de duplicata, idem; caso contrário, aceita-se 1 inserção por flush confirmado.

**Contexto**: Uma confirmação de rede perdida após a nuvem aceitar o insert faria o
firmware reenviar. Sem idempotência, geraria duplicatas.

**Alternativas consideradas**:
- `upsert` com `on conflict do update`: desnecessário (a linha é imutável); `ignore` é mais
  barato. Rejeitado o update.
- Gerar UUID de cliente por registro: custo de espaço/RAM e desnecessário — a chave natural
  já existe no schema. Rejeitado.
- Confiar só no controle local `confirmado`: não protege contra a janela “aceito na nuvem,
  confirmação perdida”. Insuficiente sozinho.

**Implicações**: O constraint `unique (tomada_id, timestamp)` em `medicoes` é
**pré-requisito** e já está no schema da 001 — a verificação de schema (decisão 6)
confirma sua presença. `Prefer: resolution=ignore-duplicates` exige que o conflito caia no
índice único.

---

## 4. Envio em lote e flag `confirmado`

**Decisão**: O buffer local marca cada registro com `confirmado=false`. Um flush seleciona
os pendentes (até um teto de tamanho de lote), envia em **uma** requisição (array JSON) e,
ao receber 2xx, marca todos como `confirmado=true` numa **única** consolidação na flash.
A purga remove só `confirmado=true` (e qualquer coisa > 7 d, confirmada ou não).

**Contexto**: Princípio III proíbe escrita por minuto. Hoje o template insere 1 a 1.

**Alternativas consideradas**:
- Persistir `confirmado` por registro imediatamente após cada 2xx: escrita frequente na
  flash. Rejeitado — consolidar em lote.
- Manter o lote só em RAM até confirmar: risco de perda em reboot antes do flush.
  Rejeitado — bufferiza-se em LittleFS (baldes por dia) e marca-se confirmado em lote.

**Implicações**: Dimensionar o teto do lote para caber na stack (40 KB) e na resposta HTTP;
flush parcial é aceitável (o restante vai no próximo ciclo). Ordenar por `timestamp` para
recuperação previsível.

---

## 5. Pull de configuração por versão e leitura do comando manual

**Decisão**: Um GET único traz `config`, `config_version` e `rele_aberto`. Adota-se a
`config` remota **somente** se `config_version_remota > config_version_local`, atualizando
a versão local; o `rele_aberto` é aplicado pelo ciclo de decisão **subordinado ao `P_max`**
(FR-010a/016). Parsing de `f32` no `config` é manual onde necessário (bug Xtensa).

**Contexto**: O dashboard é a fonte do desired state (config + comando manual). O template
já tem `buscar_config_remota()` (por versão) e `obter_rele_aberto()` — este refinamento os
mantém, mas sob auth JWT, e idealmente **funde** as duas leituras numa só requisição.

**Alternativas consideradas**:
- Duas requisições separadas (config e rele): mais rede; manter só se a fusão complicar o
  parse manual. Preferência pela fusão; fallback para duas se necessário.
- Aplicar `rele_aberto` diretamente no atuador ignorando o solver: violaria `P_max`.
  Rejeitado — o comando entra como entrada do ciclo de decisão.

**Implicações**: A escrita de `estado`/telemetria/calibração/`config` usa só colunas
permitidas ao role `device`; `rele_aberto`/`p_max`/`mac` são somente-leitura para o device
(garantido por grant de coluna + RLS).

---

## 6. Aplicação e verificação da RLS ponta-a-ponta

**Decisão**: Aplicar o `supabase-schema.sql` da 001 (idempotente) e validar com um script
de host (`scripts/verify-rls.*`) que exercita a **matriz** de `contracts/rls-verification.md`
usando JWTs reais de duas residências de teste: acessos cruzados negados, escrita de
colunas reservadas negada, `anon` negado, e operações legítimas permitidas.

**Contexto**: A RLS pode estar correta no banco mas não exercida pelo cliente; a 001 deixou
T012 (aplicar) e T020 (verificar) pendentes. Verificação automatizável aumenta a confiança
(SC-003/004).

**Alternativas consideradas**:
- Confiar na revisão manual do SQL: não cobre a interação cliente↔RLS. Insuficiente.
- Testes pgTAP no banco: bom para o lado SQL, mas não exercita o caminho HTTP/headers do
  device. Complementar, não substituto; fora do escopo mínimo.

**Implicações**: O script precisa de JWTs de teste — gerados pela Edge Function
(`provisionar` com claim-codes de teste) ou assinados localmente com o JWT secret de um
projeto de teste. Documentado no `quickstart.md`. Nunca usa segredos de produção em CI.

---

## 7. Testabilidade no host (Princípio VII) sem hardware

**Decisão**: Extrair a lógica pura em funções livres de I/O: (a) construção do corpo JSON
em lote e escape; (b) seleção de pendentes/ordenação; (c) decisão de pull por versão;
(d) decisão de renovar por margem de `exp`. Testar com `cargo test` no host.

**Contexto**: O firmware não tem testes automatizados de hardware; a lógica de
sincronização é a parte mais suscetível a regressão (idempotência, versão, expiração).

**Alternativas consideradas**:
- Só smoke test on-device: lento, não reproduzível, exige hardware. Insuficiente para a
  lógica de borda.
- Mockar o cliente HTTP inteiro: custo alto no esp-idf; preferir extrair funções puras.

**Implicações**: As funções puras não dependem de `esp-idf-svc`; ficam num módulo
testável. O caminho de I/O (HTTP real, LittleFS) continua validado por smoke test
on-device e pelo script de RLS.
