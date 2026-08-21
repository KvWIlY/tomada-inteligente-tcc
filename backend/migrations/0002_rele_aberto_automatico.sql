-- 0002 — `tomadas.rele_aberto` passa a ser TRI-ESTADO.
--
-- Bug que isto corrige: a coluna era `NOT NULL DEFAULT 1` e o INSERT do
-- provisionamento (`db::tomadas::inserir`) não a preenchia. Logo, toda tomada
-- recém-provisionada nascia com `rele_aberto = 1`, o `GET /config` devolvia
-- `true`, o firmware lia isso como "o dono mandou cortar" e o
-- `aplicar_comando_manual` forçava `Estado::Off` em TODO ciclo — a decisão do
-- MILP era calculada e jogada fora. Em modo nuvem o solver nunca mandou no relé.
-- (Passou despercebido porque em standalone `rele_manual` fica `None` e a
-- automação funciona normalmente.)
--
-- Causa raiz: o contrato só tinha dois valores (cortar / manter ligado) e faltava
-- o terceiro, que é o normal — automático. O firmware já sabia representá-lo
-- (`Option<bool>`); era o backend que nunca enviava `None`.
--
-- Semântica a partir daqui:
--   NULL  = automático   → o MILP decide (padrão de uma tomada nova)
--   TRUE  = cortar       → trava explícita do dono, até ele desfazer
--   FALSE = usar agora   → o firmware abre uma janela temporária de 30 min e,
--                          ao fim dela, devolve esta coluna a NULL sozinho
--                          (campo `liberar_manual` do snapshot em POST /telemetria)
--
-- "Sempre ligada" (geladeira, roteador) NÃO é esta coluna: é `is_critical` no
-- JSON de `config`, que vira W_i = M dentro do modelo.

ALTER TABLE tomadas
  MODIFY COLUMN rele_aberto TINYINT(1) NULL DEFAULT NULL;

-- Linhas existentes voltam para automático. Não é possível distinguir um `1`
-- deliberado (dono clicou "desligar") do `1` que veio do default antigo, e entre
-- os dois erros o seguro é devolver o controle ao otimizador em vez de manter
-- tomadas travadas em OFF sem ninguém ter pedido. Um corte manual de verdade é
-- re-emitido com um clique no dashboard.
UPDATE tomadas SET rele_aberto = NULL;
