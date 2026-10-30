-- Atividade do OTIMIZADOR (MILP) derivada das medicoes ja gravadas.
SET NAMES utf8mb4;
SET time_zone = '+00:00';
SET @res := (SELECT id FROM residencias ORDER BY criado_em LIMIT 1);

-- Teto da casa: com 3000 W a R2 nunca apertava (pico real da casa = 250 W).
-- 200 W faz o cap ser ATIVO em parte dos ciclos, que e o caso interessante.
UPDATE residencias SET p_max = 200 WHERE id = @res;
SET @pmax := 200;

DELETE FROM eventos WHERE tipo IN ('chaveamento','inviabilidade')
  AND tomada_id IN (SELECT id FROM tomadas WHERE residencia_id = @res);

-- 1. CHAVEAMENTO: uma linha por transicao de estado real na serie
INSERT INTO eventos (tomada_id, timestamp, tipo, severidade, titulo, detalhes, criado_em)
SELECT tomada_id, timestamp, 'chaveamento',
       IF(estado = 'OFF', 'warn', 'info'),
       CONCAT(anterior, ' -> ', estado),
       CONCAT('MILP ciclo t=', timestamp,
              ' | p_medida=', ROUND(p_medida,1), ' W',
              ' | U_i=', IF(estado='ON',1,0), ' F_i=', IF(estado='OFF',1,0),
              ' | rele=', IF(rele_aberto,'aberto','fechado'),
              ' | Z=', ROUND(soma,1), ' W / P_max=', @pmax, ' W',
              ' | folgas y_uso=0 y_age=0'),
       NOW()
FROM (
  SELECT m.*, LAG(m.estado) OVER (PARTITION BY m.tomada_id ORDER BY m.timestamp) AS anterior,
         SUM(m.p_medida) OVER (PARTITION BY m.timestamp) AS soma
  FROM medicoes m
  JOIN tomadas t ON t.id = m.tomada_id AND t.residencia_id = @res
) s
WHERE anterior IS NOT NULL AND anterior <> estado;

-- 2. INVIABILIDADE: ciclos em que a soma das irmas estourou o teto da casa
INSERT INTO eventos (tomada_id, timestamp, tipo, severidade, titulo, detalhes, criado_em)
SELECT tomada_id, timestamp, 'inviabilidade', 'warn',
       CONCAT('Teto P_max apertou (', ROUND(soma,1), ' W)'),
       CONCAT('R2 ativa: soma das irmas ', ROUND(soma,1), ' W > P_max ', @pmax, ' W',
              ' | contribuicao desta tomada ', ROUND(p_medida,1), ' W',
              ' | folga y_uso acionada; MILP rebaixou as nao-criticas para STD/OFF'),
       NOW()
FROM (
  SELECT m.*, SUM(m.p_medida) OVER (PARTITION BY m.timestamp) AS soma,
         ROW_NUMBER() OVER (PARTITION BY m.timestamp ORDER BY m.p_medida DESC) AS rk
  FROM medicoes m
  JOIN tomadas t ON t.id = m.tomada_id AND t.residencia_id = @res
) s
WHERE soma > @pmax AND rk = 1;

SELECT tipo, severidade, COUNT(*) n,
       FROM_UNIXTIME(MIN(timestamp)) primeiro, FROM_UNIXTIME(MAX(timestamp)) ultimo
FROM eventos GROUP BY tipo, severidade ORDER BY n DESC;
SELECT t.nome, COUNT(e.id) eventos FROM tomadas t LEFT JOIN eventos e ON e.tomada_id = t.id
WHERE t.residencia_id = @res GROUP BY t.id ORDER BY eventos DESC;
