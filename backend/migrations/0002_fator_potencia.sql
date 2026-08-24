-- Fator de potencia por medicao (2026-08-24).
--
-- Motivo: ate aqui `p_medida` era a potencia APARENTE (`Vrms * Irms`, VA) apresentada
-- como se fosse watt. Para carga resistiva da no mesmo; para as cargas-alvo deste
-- sistema — fontes chaveadas em standby, exatamente o consumo fantasma que o TCC
-- pretende cortar — o fator de potencia e baixo e a aparente SUPERESTIMA o consumo
-- real. Com isso, `cstd`/`con` (autocalibrados pelo firmware a partir de `p_medida`)
-- e o teto `p_max` da R2 estavam todos em VA, e a economia relatada saia inflada.
--
-- O firmware passou a calcular a potencia ATIVA — `media(v_k * i_k)` sobre a mesma
-- janela interleaved que ja existia — e `p_medida` agora carrega W de verdade. Esta
-- coluna guarda o `P/S` da mesma janela.
--
-- Por que FP e nao `vrms`/`irms`: fator de potencia NAO e recuperavel a partir dos dois
-- RMS (precisa da fase, que so existe durante a amostragem). Guardar V e I nao
-- permitiria reconstruir isto depois; guardar o FP sim, e com um float em vez de dois.
--
-- NULL = medicao anterior a esta mudanca (valor em VA, FP desconhecido). Nao ha
-- backfill possivel: a fase daquelas janelas nao foi gravada. Quem consumir a serie
-- historica precisa tratar o NULL como "sem informacao", nunca como zero.

ALTER TABLE medicoes
  ADD COLUMN fator_potencia FLOAT NULL DEFAULT NULL AFTER p_medida;
