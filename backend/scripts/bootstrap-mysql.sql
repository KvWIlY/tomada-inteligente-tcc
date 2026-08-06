-- Bootstrap do MySQL local para o backend (feature 004).
-- Roda UMA vez, como root. Cria os bancos e a conta de aplicação `tcc_app`.
-- A conta root do MySQL nunca é usada pelo backend (Princípio VIII).
--
--   mysql -u root -p < backend/scripts/bootstrap-mysql.sql
--
-- Bancos: `tcc` (dev/execução) e `tcc_test` (cargo test — os testes escrevem
-- de verdade, então ficam isolados do banco de dev).

CREATE DATABASE IF NOT EXISTS tcc      CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci;
CREATE DATABASE IF NOT EXISTS tcc_test CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci;

CREATE USER IF NOT EXISTS 'tcc_app'@'localhost'
  IDENTIFIED BY 'dNScMvDhMxlqc925u0o5E7AS';
CREATE USER IF NOT EXISTS 'tcc_app'@'127.0.0.1'
  IDENTIFIED BY 'dNScMvDhMxlqc925u0o5E7AS';

GRANT ALL PRIVILEGES ON tcc.*      TO 'tcc_app'@'localhost', 'tcc_app'@'127.0.0.1';
GRANT ALL PRIVILEGES ON tcc_test.* TO 'tcc_app'@'localhost', 'tcc_app'@'127.0.0.1';

FLUSH PRIVILEGES;
SELECT 'bancos tcc/tcc_test e usuario tcc_app prontos' AS status;
