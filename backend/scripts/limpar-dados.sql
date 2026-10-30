-- Esvazia o banco `tcc` SEM derrubar o schema nem as migrations.
--
--   "C:\Program Files\MySQL\MySQL Server 8.0\bin\mysql.exe" -u tcc_app -p tcc < backend/scripts/limpar-dados.sql
--
-- Existe porque dev e `cargo test` compartilham o mesmo banco (nao ha mais
-- `tcc_test`): os testes de integracao gravam usuarios/residencias/tomadas de
-- verdade, e este script e o botao de "zerar" quando o lixo incomodar.
--
-- NAO mexe em `_sqlx_migrations` - o backend nao vai tentar re-aplicar nada no
-- proximo boot. Para recomecar TAMBEM o schema, use DROP DATABASE + bootstrap.
-- Isto e irreversivel: faca o dump antes se os dados importarem
--   mysqldump -u tcc_app -p --databases tcc --single-transaction > backup.sql

SET FOREIGN_KEY_CHECKS = 0;

TRUNCATE TABLE medicoes;
TRUNCATE TABLE eventos;
TRUNCATE TABLE device_credentials;
TRUNCATE TABLE provisioning_claims;
TRUNCATE TABLE residencia_secrets;
TRUNCATE TABLE tomadas;
TRUNCATE TABLE residencias;
TRUNCATE TABLE usuarios;

SET FOREIGN_KEY_CHECKS = 1;

SELECT 'banco tcc esvaziado (schema e migrations intactos)' AS status;
