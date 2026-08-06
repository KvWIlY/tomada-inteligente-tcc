-- Schema MySQL 8 — Backend Próprio (feature 004-backend-actix-mysql)
-- Porte do schema autoritativo de 001 (Postgres) para MySQL.
-- Fonte única de verdade = este banco, acessado APENAS pelo backend (Constituição v2.0.0):
--   * Princípio VI: sem BaaS; clientes não acessam o banco direto (só via API do backend).
--   * Princípio VIII: identidade = MAC (coluna `mac` única); autenticação = JWT por-device
--     emitido pelo backend; segredo de assinatura só no backend; autorização DEFAULT-DENY
--     implementada no BACKEND (não há RLS/role por cliente no MySQL). A conta admin do MySQL
--     nunca é exposta ao firmware nem ao frontend.
-- Idempotente onde possível (CREATE TABLE IF NOT EXISTS). Aplicar via migrations do backend.

SET NAMES utf8mb4;
SET time_zone = '+00:00';  -- backend normaliza tudo para UTC

-- ============================================================
-- 1. usuarios  (NOVO — substitui auth.users do Supabase)
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
-- 2. residencias  (dono único; teto P_max — R2)
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
--    `mac` = identidade de hardware (única, imutável).
-- ============================================================
CREATE TABLE IF NOT EXISTS tomadas (
  id                   CHAR(36)     NOT NULL,
  residencia_id        CHAR(36)     NOT NULL,
  mac                  VARCHAR(17)  NOT NULL,           -- AA:BB:CC:DD:EE:FF
  nome                 VARCHAR(255) NOT NULL,
  estado               ENUM('OFF','STD','ON') NOT NULL DEFAULT 'OFF',
  rele_aberto          TINYINT(1)   NOT NULL DEFAULT 1, -- comando manual (só o dono)
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
-- 4. medicoes  (série temporal, enviada em lote idempotente)
--    UNIQUE(tomada_id, timestamp) => base da idempotência (FR-009).
-- ============================================================
CREATE TABLE IF NOT EXISTS medicoes (
  id           BIGINT     NOT NULL AUTO_INCREMENT,
  tomada_id    CHAR(36)   NOT NULL,
  timestamp    BIGINT     NOT NULL,              -- epoch (SNTP)
  p_medida     FLOAT      NOT NULL,              -- >= 0 (validado no backend)
  estado       ENUM('OFF','STD','ON') NOT NULL,
  rele_aberto  TINYINT(1) NOT NULL,
  criado_em    DATETIME   NOT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY uq_medicoes_tomada_ts (tomada_id, timestamp),
  KEY idx_medicoes_tomada_ts (tomada_id, timestamp DESC),
  CONSTRAINT fk_medicoes_tomada FOREIGN KEY (tomada_id)
    REFERENCES tomadas(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 5. eventos  (observabilidade — Princípio VII)
-- ============================================================
CREATE TABLE IF NOT EXISTS eventos (
  id           BIGINT     NOT NULL AUTO_INCREMENT,
  tomada_id    CHAR(36)   NULL,
  timestamp    BIGINT     NOT NULL,
  tipo         VARCHAR(40) NOT NULL,
  severidade   ENUM('info','warn','erro') NOT NULL DEFAULT 'info',
  titulo       VARCHAR(255) NOT NULL,
  detalhes     TEXT       NULL,                  -- entradas + Z + estados + folgas
  criado_em    DATETIME   NOT NULL,
  PRIMARY KEY (id),
  KEY idx_eventos_tomada_ts (tomada_id, timestamp DESC),
  CONSTRAINT fk_eventos_tomada FOREIGN KEY (tomada_id)
    REFERENCES tomadas(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ============================================================
-- 6. device_credentials  (segredo durável por-device; só hash)
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
-- 7. provisioning_claims  (claim-codes; só hash; uso único; expira)
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
-- 8. residencia_secrets  (chave HMAC ESP-NOW por residência)
--    Lida APENAS pelo backend; entregue só na resposta de /provisionar.
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
-- Notas de autorização (NÃO há RLS no MySQL):
--   Todo o isolamento default-deny é feito no BACKEND (middleware + filtros por claim):
--   - device (claim tomada_id/residencia_id): lê tomadas/medicoes/eventos da sua residência;
--     escreve só a PRÓPRIA tomada (estado, cstd, con, p_medida_atual, ultimo_uso_ativo_ts,
--     config, config_version) + insere medicoes/eventos da própria tomada. NUNCA toca
--     rele_aberto, p_max, mac, residencia_id, device_credentials, provisioning_claims,
--     residencia_secrets.
--   - usuário (owner_id): CRUD só das próprias residências/tomadas; escreve rele_aberto,
--     p_max, config, nome; cria/consulta claim-codes; revoga devices.
--   Ver contracts/backend-api.md e contracts/provisioning-api.md.
