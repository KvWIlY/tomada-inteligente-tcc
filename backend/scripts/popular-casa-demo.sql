-- Popula o banco `tcc` com 4 tomadas de aparelhos diferentes + 7 dias de medicoes.
SET NAMES utf8mb4;
SET time_zone = '+00:00';
SET @res := (SELECT id FROM residencias ORDER BY criado_em LIMIT 1);
SET SESSION cte_max_recursion_depth = 20000;

-- as 3 tomadas de placeholder (sem nenhuma medicao) saem
DELETE FROM tomadas WHERE residencia_id = @res;

SET @t1 := UUID(); SET @t2 := UUID(); SET @t3 := UUID(); SET @t4 := UUID();

INSERT INTO tomadas (id, residencia_id, mac, nome, estado, rele_aberto, cstd, con,
                     p_medida_atual, ultimo_uso_ativo_ts, config, config_version, criado_em)
VALUES
 (@t1, @res, 'AA:BB:CC:DD:EE:01', 'Geladeira',     'ON',  NULL,   2.0, 120.0, 118.4, UNIX_TIMESTAMP(), JSON_OBJECT(
    'is_critical', TRUE,  'w_i', 500,  't_limite', 1800, 'p_limiar_detectado_auto', 60.0,
    'schedule', JSON_ARRAY(true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true)), 1, NOW()),
 (@t2, @res, 'AA:BB:CC:DD:EE:02', 'TV da Sala',    'STD', NULL,   8.0,  90.0,   8.3, UNIX_TIMESTAMP() - 7200, JSON_OBJECT(
    'is_critical', FALSE, 'w_i', 10,   't_limite', 600,  'p_limiar_detectado_auto', 40.0,
    'schedule', JSON_ARRAY(false,false,false,false,false,false,false,false,false,false,false,false,true,true,false,false,false,false,true,true,true,true,true,false)), 1, NOW()),
 (@t3, @res, 'AA:BB:CC:DD:EE:03', 'Carregadores',  'STD', NULL,   2.5,  18.0,   2.6, UNIX_TIMESTAMP() - 28800, JSON_OBJECT(
    'is_critical', FALSE, 'w_i', 1,    't_limite', 600,  'p_limiar_detectado_auto', 8.0,
    'schedule', JSON_ARRAY(true,true,true,true,true,true,true,false,false,false,false,false,false,false,false,false,false,false,false,false,false,false,true,true)), 1, NOW()),
 (@t4, @res, 'AA:BB:CC:DD:EE:04', 'Roteador',      'ON',  NULL,   9.0,   9.0,   9.1, UNIX_TIMESTAMP(), JSON_OBJECT(
    'is_critical', TRUE,  'w_i', 1000, 't_limite', 3600, 'p_limiar_detectado_auto', 4.0,
    'schedule', JSON_ARRAY(true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true)), 1, NOW());

INSERT INTO device_credentials (tomada_id, secret_hash, criado_em)
VALUES (@t1, SHA2(CONCAT('seed-', @t1), 256), NOW()),
       (@t2, SHA2(CONCAT('seed-', @t2), 256), NOW()),
       (@t3, SHA2(CONCAT('seed-', @t3), 256), NOW()),
       (@t4, SHA2(CONCAT('seed-', @t4), 256), NOW());

INSERT INTO residencia_secrets (residencia_id, residencia_key, criado_em)
VALUES (@res, SHA2(UUID(), 256), NOW())
ON DUPLICATE KEY UPDATE residencia_key = residencia_key;

SET @fim := UNIX_TIMESTAMP() DIV 600 * 600;

INSERT INTO medicoes (tomada_id, timestamp, p_medida, fator_potencia, estado, rele_aberto, criado_em)
WITH RECURSIVE n(k) AS (SELECT 0 UNION ALL SELECT k+1 FROM n WHERE k < 1007),
grade AS (
  SELECT @fim - k*600 AS ts,
         HOUR(FROM_UNIXTIME(@fim - k*600)) AS h,
         (@fim - k*600) DIV 600 % 6 AS slot,
         RAND(k) AS r
  FROM n
)
SELECT @t1, ts, IF(slot < 2, 120.0*(0.96+0.08*r), 2.0*(0.9+0.2*r)),
       IF(slot < 2, 0.85, 0.42), IF(slot < 2,'ON','STD'), 0, NOW() FROM grade
UNION ALL
SELECT @t2, ts, IF(h IN (12,13,18,19,20,21,22), 90.0*(0.93+0.14*r), 8.0*(0.9+0.2*r)),
       IF(h IN (12,13,18,19,20,21,22), 0.62, 0.38),
       IF(h IN (12,13,18,19,20,21,22),'ON','STD'), 0, NOW() FROM grade
UNION ALL
SELECT @t3, ts,
       CASE WHEN h < 6 OR h >= 22 THEN 18.0*(0.9+0.2*r)
            WHEN h BETWEEN 9 AND 17 THEN 0.0
            ELSE 2.5*(0.85+0.3*r) END,
       CASE WHEN h < 6 OR h >= 22 THEN 0.55 WHEN h BETWEEN 9 AND 17 THEN NULL ELSE 0.31 END,
       CASE WHEN h < 6 OR h >= 22 THEN 'ON' WHEN h BETWEEN 9 AND 17 THEN 'OFF' ELSE 'STD' END,
       IF(h BETWEEN 9 AND 17, 1, 0), NOW() FROM grade
UNION ALL
SELECT @t4, ts, 9.0*(0.97+0.06*r), 0.95, 'ON', 0, NOW() FROM grade;

INSERT INTO eventos (tomada_id, timestamp, tipo, severidade, titulo, detalhes, criado_em) VALUES
 (@t1, @fim - 86400*6, 'cold_start',   'info', 'Boot da tomada',            'Geladeira: boot, LittleFS montado, SNTP pendente', NOW()),
 (@t2, @fim - 86400*5, 'chaveamento',  'info', 'STD -> ON',                 'TV da Sala: uso detectado (p_medida 88 W > p_limiar 40 W)', NOW()),
 (@t3, @fim - 86400*3, 'chaveamento',  'info', 'STD -> OFF',                'Carregadores: MILP cortou o standby (w_i=1, fora da janela)', NOW()),
 (@t3, @fim - 86400*2, 'inviabilidade','warn', 'Teto P_max apertou',        'Soma das irmas 3120 W > p_max 3000 W; folga y_uso acionada', NOW()),
 (@t4, @fim - 86400*1, 'config',       'info', 'Config adotada (v1)',       'Roteador: is_critical=true, w_i=1000', NOW()),
 (@t2, @fim - 3600,    'sync_error',   'warn', 'Falha ao enviar telemetria','TV da Sala: POST /telemetria timeout; 6 medicoes bufferizadas', NOW());

SELECT t.nome, t.mac, t.estado, t.cstd, t.con, COUNT(m.id) AS medicoes,
       ROUND(MIN(m.p_medida),1) AS p_min, ROUND(MAX(m.p_medida),1) AS p_max
FROM tomadas t LEFT JOIN medicoes m ON m.tomada_id = t.id
WHERE t.residencia_id = @res GROUP BY t.id ORDER BY t.mac;
