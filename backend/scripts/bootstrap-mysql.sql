-- Bootstrap do MySQL local para o backend (features 004/005).
-- Roda UMA vez, como root. Cria o banco e a conta de aplicacao `tcc_app`.
-- A conta root do MySQL nunca e usada pelo backend (Principio VIII).
--
--   mysql -u root -p < backend/scripts/bootstrap-mysql.sql
--
-- Banco UNICO: `tcc`. Desde 2026-08-23 nao existe mais `tcc_test` — dev e
-- `cargo test` batem no mesmo banco (decisao do dono: limpar sob demanda com
-- scripts/limpar-dados.sql em vez de manter dois bancos em sincronia).
-- Os testes de integracao escrevem de verdade: `usuarios`/`residencias`/`tomadas`
-- com nomes "de teste" acumulam ate alguem rodar o limpar-dados.

CREATE DATABASE IF NOT EXISTS tcc CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci;

CREATE USER IF NOT EXISTS 'tcc_app'@'localhost'
  IDENTIFIED BY 'dNScMvDhMxlqc925u0o5E7AS';
CREATE USER IF NOT EXISTS 'tcc_app'@'127.0.0.1'
  IDENTIFIED BY 'dNScMvDhMxlqc925u0o5E7AS';

GRANT ALL PRIVILEGES ON tcc.* TO 'tcc_app'@'localhost', 'tcc_app'@'127.0.0.1';

FLUSH PRIVILEGES;
SELECT 'banco tcc e usuario tcc_app prontos' AS status;
