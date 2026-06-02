-- Modo demo "conectar e usar" (sem login): permite criar uma residência padrão
-- ("Casa Demo") sem um usuário autenticado dono. owner_id continua referenciando
-- auth.users quando houver dono (dashboard humano), mas passa a ser opcional.
--
-- Impacto na RLS: as policies humanas chaveiam em `owner_id = auth.uid()`; com owner_id
-- NULL elas simplesmente não casam (sem dono humano), o que é o esperado no modo demo.

alter table residencias alter column owner_id drop not null;
