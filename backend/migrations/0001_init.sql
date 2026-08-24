-- Schema MySQL 8 — banco unico `tcc` (backend proprio, features 004/005).
--
-- Reconstruido do zero em 2026-08-23: os bancos `tcc` e `tcc_test` foram descartados
-- e fundidos num so. O par antigo (0001_init + 0002_rele_aberto_automatico) virou este
-- arquivo unico, ja com `rele_aberto` TRI-ESTADO desde o nascimento — o bug em que toda
-- tomada nova nascia com `rele_aberto = 1` e o firmware lia isso como "o dono mandou
-- cortar" (jogando fora a decisao do MILP em todo ciclo) nao existe mais no schema.
-- Backup do banco antigo: ok/backup-db/tcc_2026-08-23.sql
--
-- Fonte unica de verdade = este banco, acessado APENAS pelo backend (Constituicao v2.0.0):
--   * Principio VI: sem BaaS; clientes nao acessam o banco direto (so via API do backend).
--   * Principio VIII: identidade = MAC (coluna `mac` unica); autenticacao = JWT por-device
--     emitido pelo backend; segredo de assinatura so no backend; autorizacao DEFAULT-DENY
--     implementada no BACKEND (nao ha RLS/role por cliente no MySQL). A conta admin do
--     MySQL nunca e exposta ao firmware nem ao frontend.
--
-- Modelo de dominio (a forma do sistema hoje):
--   1 usuario -> 1..N residencias — na pratica UMA casa; o schema nao trava isso
--                (sem UNIQUE em owner_id), para nao fechar a porta de varias casas.
--   1 casa    -> 1..N tomadas — no minimo 1 para a casa fazer sentido. Isso NAO vira
--                constraint: a casa e criada ANTES da primeira tomada existir (a tomada
--                so nasce quando o claim-code e usado no portal AP). A regra vive na
--                aplicacao, nao no banco.
--   1 tomada  -> suas medicoes (consumo), seus eventos e sua credencial de dispositivo.
--
-- Idempotente onde possivel (CREATE TABLE IF NOT EXISTS). Aplicado via sqlx::migrate!.

SET NAMES utf8mb4;
SET time_zone = '+00:00';  -- backend normaliza tudo para UTC

-- ============================================================
-- 1. usuarios  (dono da casa; login do dashboard)
-- ============================================================
CREATE TABLE IF NOT EXISTS usuarios (
  id          CHAR(36)     NOT NULL,
  email       VARCHAR(255) NOT NULL,
  senha_hash  VARCHAR(255) NOT NULL,           -- Argon2id (nunca a senha)
  criado_em   DATETIME     NOT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY uq_usuarios_email (email)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 2. residencias  (a casa; dono unico; teto P_max — R2 do modelo)
-- ============================================================
CREATE TABLE IF NOT EXISTS residencias (
  id          CHAR(36)     NOT NULL,
  owner_id    CHAR(36)     NOT NULL,
  nome        VARCHAR(255) NOT NULL,
  p_max       INT          NOT NULL,           -- W, > 0 (validado no backend)
  criado_em   DATETIME     NOT NULL,
  PRIMARY KEY (id),
  KEY idx_residencias_owner (owner_id),
  CONSTRAINT fk_residencias_owner FOREIGN KEY (owner_id)
    REFERENCES usuarios(id) ON DELETE CASCADE,
  CONSTRAINT chk_residencias_pmax CHECK (p_max > 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 3. tomadas  (config do modelo + telemetria compartilhada)
--    `mac` = identidade de hardware (unica, imutavel).
-- ============================================================
CREATE TABLE IF NOT EXISTS tomadas (
  id                   CHAR(36)     NOT NULL,
  residencia_id        CHAR(36)     NOT NULL,
  mac                  VARCHAR(17)  NOT NULL,           -- AA:BB:CC:DD:EE:FF
  nome                 VARCHAR(255) NOT NULL,
  estado               ENUM('OFF','STD','ON') NOT NULL DEFAULT 'OFF',
  -- TRI-ESTADO — comando manual do dono (so o dono escreve):
  --   NULL  = automatico  -> o MILP decide. E o padrao de uma tomada nova.
  --   TRUE  = cortar      -> trava explicita do dono, ate ele desfazer.
  --   FALSE = usar agora  -> o firmware abre uma janela temporaria de 30 min e, ao fim
  --                          dela, devolve esta coluna a NULL sozinho (campo
  --                          `liberar_manual` do snapshot em POST /telemetria).
  -- "Sempre ligada" (geladeira, roteador) NAO e esta coluna: e `is_critical` dentro do
  -- JSON de `config`, que vira W_i = M no modelo.
  rele_aberto          TINYINT(1)   NULL DEFAULT NULL,
  cstd                 FLOAT        NOT NULL DEFAULT 0,  -- C_{i,STD} (W)
  con                  FLOAT        NOT NULL DEFAULT 0,  -- C_{i,ON}  (W)
  p_medida_atual       FLOAT        NOT NULL DEFAULT 0,
  ultimo_uso_ativo_ts  BIGINT       NOT NULL DEFAULT 0,
  config               JSON         NOT NULL,
  config_version       BIGINT       NOT NULL DEFAULT 0,
  criado_em            DATETIME     NOT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY uq_tomadas_mac (mac),
  KEY idx_tomadas_residencia (residencia_id),
  CONSTRAINT fk_tomadas_residencia FOREIGN KEY (residencia_id)
    REFERENCES residencias(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
-- Estrutura de `config` (JSON): { is_critical, w_i, p_max, t_limite,
--   schedule[24], pi_historico[24], p_limiar_detectado_auto }

-- ============================================================
-- 4. medicoes  (consumo da tomada; serie temporal enviada em lote idempotente)
--    UNIQUE(tomada_id, timestamp) => base da idempotencia (FR-009).
-- ============================================================
CREATE TABLE IF NOT EXISTS medicoes (
  id           BIGINT     NOT NULL AUTO_INCREMENT,
  tomada_id    CHAR(36)   NOT NULL,
  timestamp    BIGINT     NOT NULL,              -- epoch (SNTP)
  p_medida     FLOAT      NOT NULL,              -- >= 0 (validado no backend)
  estado       ENUM('OFF','STD','ON') NOT NULL,
  rele_aberto  TINYINT(1) NOT NULL,              -- estado FISICO no instante da medida
  criado_em    DATETIME   NOT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY uq_medicoes_tomada_ts (tomada_id, timestamp),
  KEY idx_medicoes_tomada_ts (tomada_id, timestamp DESC),
  CONSTRAINT fk_medicoes_tomada FOREIGN KEY (tomada_id)
    REFERENCES tomadas(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 5. eventos  (observabilidade da tomada — Principio VII)
-- ============================================================
CREATE TABLE IF NOT EXISTS eventos (
  id           BIGINT      NOT NULL AUTO_INCREMENT,
  tomada_id    CHAR(36)    NULL,
  timestamp    BIGINT      NOT NULL,
  tipo         VARCHAR(40) NOT NULL,
  severidade   ENUM('info','warn','erro') NOT NULL DEFAULT 'info',
  titulo       VARCHAR(255) NOT NULL,
  detalhes     TEXT        NULL,                 -- entradas + Z + estados + folgas
  criado_em    DATETIME    NOT NULL,
  PRIMARY KEY (id),
  KEY idx_eventos_tomada_ts (tomada_id, timestamp DESC),
  CONSTRAINT fk_eventos_tomada FOREIGN KEY (tomada_id)
    REFERENCES tomadas(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 6. device_credentials  (credencial duravel da tomada; so o hash)
--    Lida APENAS pelo backend (nunca por endpoint de cliente).
-- ============================================================
CREATE TABLE IF NOT EXISTS device_credentials (
  tomada_id         CHAR(36)    NOT NULL,
  secret_hash       VARCHAR(64) NOT NULL,        -- sha256(device_secret) hex
  criado_em         DATETIME    NOT NULL,
  revoked_at        DATETIME    NULL,            -- not null => revogada
  ultima_renovacao  DATETIME    NULL,
  PRIMARY KEY (tomada_id),
  CONSTRAINT fk_devcred_tomada FOREIGN KEY (tomada_id)
    REFERENCES tomadas(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 7. provisioning_claims  (claim-codes da casa; so o hash; uso unico; expira)
-- ============================================================
CREATE TABLE IF NOT EXISTS provisioning_claims (
  id            CHAR(36)    NOT NULL,
  residencia_id CHAR(36)    NOT NULL,
  code_hash     VARCHAR(64) NOT NULL,            -- sha256(claim_code)
  expira_em     DATETIME    NOT NULL,
  usado_em      DATETIME    NULL,
  criado_em     DATETIME    NOT NULL,
  PRIMARY KEY (id),
  KEY idx_claims_residencia (residencia_id),
  CONSTRAINT fk_claims_residencia FOREIGN KEY (residencia_id)
    REFERENCES residencias(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 8. residencia_secrets  (chave HMAC do ESP-NOW, uma por casa)
--    Lida APENAS pelo backend; entregue so na resposta de /provisionar e rotacionada
--    na mesma transacao do DELETE /tomadas/{id}.
-- ============================================================
CREATE TABLE IF NOT EXISTS residencia_secrets (
  residencia_id  CHAR(36)  NOT NULL,
  residencia_key CHAR(64)  NOT NULL,             -- 32 bytes hex
  criado_em      DATETIME  NOT NULL,
  PRIMARY KEY (residencia_id),
  CONSTRAINT fk_ressecret_residencia FOREIGN KEY (residencia_id)
    REFERENCES residencias(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- Notas de autorizacao (NAO ha RLS no MySQL):
--   Todo o isolamento default-deny e feito no BACKEND (middleware + filtros por claim):
--   - device (claim tomada_id/residencia_id): le tomadas/medicoes/eventos da sua
--     residencia; escreve so a PROPRIA tomada (estado, cstd, con, p_medida_atual,
--     ultimo_uso_ativo_ts, config, config_version) + insere medicoes/eventos da propria
--     tomada. NUNCA toca p_max, mac, residencia_id, device_credentials,
--     provisioning_claims nem residencia_secrets; em `rele_aberto` a unica escrita
--     permitida e o `liberar_manual`, que so devolve a coluna a NULL.
--   - usuario (owner_id): CRUD so das proprias residencias/tomadas; escreve rele_aberto,
--     p_max, config, nome; cria/consulta claim-codes; revoga devices.
--   Ver contracts/backend-api.md e contracts/provisioning-api.md.
