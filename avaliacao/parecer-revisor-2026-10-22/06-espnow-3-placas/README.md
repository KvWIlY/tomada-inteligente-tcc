# 06 - Três placas trocando heartbeats por ESP-NOW (item 29)

## Por que

O parecer aponta que não houve teste de perda de pacotes nem de coordenação real: tudo era
simulado no PC, com as tomadas vendo as vizinhas "pelo anúncio do passo anterior". Aqui três
ESP32 de verdade fazem a coordenação pelo rádio, sem servidor e sem roteador.

## Como

Três placas ESP32 rev 3.1 num hub USB do PC, cada uma no papel de uma tomada do cenário 6 (pasta
04), escolhido pelo MAC:

| Porta | MAC | Papel | Potência em uso | W_i | Em uso dentro de cada volta de 6 min |
|---|---|---|---|---|---|
| COM3 | `e8:6b:ea:d0:ad:c0` | ferro | 1.000 W | 3 | 20-200 s |
| COM7 | `a8:42:e3:a7:db:c8` | air fryer | 1.500 W | 10 | 60-240 s |
| COM8 | `a0:a3:b3:ab:24:ac` | micro-ondas | 1.400 W | 10 | 120-150 s |

Teto de 2.540 W. Ferro e air fryer juntos cabem (2.500 W); quando o micro-ondas chega, os três
somam 3.900 W e alguém precisa sair. São 5 voltas em 30 min.

`firmware/examples/espnow_teste.rs`, em cada placa:

- Wi-Fi em modo estação, sem conectar a nada, no canal 1, e ESP-NOW em broadcast. Sem NVS.
- A cada 10 s, com fase aleatória entre as placas (como numa casa de verdade): lê o consumo
  sintético (0 W com o relé aberto), aperta "Usar Agora" se o aparelho está cortado e em uso (como
  o simulador), monta as vizinhas vivas, roda o `Ciclo::decidir` do firmware e difunde o
  heartbeat v6 assinado (HMAC-SHA256 com uma chave de teste).
- Do firmware: `Heartbeat` (formato, HMAC, verificação), `replay::aceitar`, `Ciclo`, `uso_agora`.
  Copiada do `espnow.rs`: a tabela de vizinhas e o `montar_coordenacao` (vizinha sem heartbeat há
  mais de 30 s vira carga fixa com o último consumo anunciado), com contadores de quadros
  recebidos e perdidos a partir do `contador` do heartbeat.
- **Falha simulada:** o ferro fica sem transmitir de 1.200 a 1.290 s (continua recebendo e
  decidindo).

O PC (`captura.ps1`) reinicia as três placas no mesmo instante pelo RTS, para a rotina andar
junta, e grava cada linha das seriais com o carimbo do PC. `analise.py` monta uma linha do tempo
comum pelo uptime e calcula o consumo real somado segundo a segundo. As placas ficaram alinhadas a
50 ms (`alinhamento` no `resultado.txt`).

## Resultado

(`resultado.txt`)

- **Nenhum quadro perdido** nos 6 sentidos: cada placa recebeu todos os heartbeats que as outras
  mandaram (175, 176 e 166, o ferro menos por causa dos 90 s mudo).
- **Decisão:** mediana de 190 ms, máximo de 1,25 s, com 3 tomadas e o firmware como está
  (`opt-level = 0`). Bate com a bancada (pasta 05).
- **Teto:** o consumo real passou de 2.540 W em 69 s ao todo, em 5 rajadas (uma por volta), de no
  máximo 20 s, com pico de +1.360 W (os três ligados juntos).
- **Quem saiu:** sempre a air fryer, 146 s de uso negado ao todo, ~30 s por volta. Ferro e
  micro-ondas nunca foram cortados. É o mesmo comportamento do simulador (pasta 04): o MILP corta o
  maior aparelho, não o de menor W_i.
- **Falha do ferro:** a air fryer e o micro-ondas continuaram vendo o ferro até o TTL de 30 s
  (1.212 s) e, a partir de 1.223 s, passaram a contá-lo como 1.000 W fixos (o último consumo que
  ele anunciou). Quando ele voltou a transmitir (1.305 s), voltou a ser variável de decisão. O teto
  foi respeitado durante a falha.

A dinâmica de uma volta (rastro das três seriais):

| Uptime | Ferro | Air fryer | Micro-ondas | O que aconteceu |
|---|---|---|---|---|
| 112 s | ON | ON | STD | ferro e air fryer cabem juntos |
| 121-122 s | ON | ON | ON | o micro-ondas chega; a air fryer decide 1 s depois, antes de o heartbeat dele chegar |
| 133 s | ON | OFF | ON | um ciclo depois a air fryer se corta (a maior) |
| 155 s | ON | OFF | STD | o micro-ondas terminou; a air fryer ainda viu o heartbeat antigo dele |
| ~166 s | ON | ON | STD | a air fryer volta |

## Leitura

- A coordenação sem servidor funciona no rádio real: zero perda a curta distância, e a degradação
  segura (vizinha sumida vira carga fixa) acontece como o firmware prevê.
- As rajadas acima do teto (até 20 s) vêm do atraso de um ciclo entre as tomadas, agravado pela
  fase aleatória: quem decide logo antes de o heartbeat da vizinha chegar só vê a mudança no ciclo
  seguinte. A 1,5 x a corrente nominal, um disjuntor termomagnético costuma levar
  mais que 20 s para desarmar, mas isso precisa ser conferido na curva do disjuntor antes de ir
  para o texto.

## Limitações

As placas não estavam conectadas a roteador nenhum, então o rádio ficou recebendo o tempo todo.
No firmware de verdade, conectado ao Wi-Fi da casa, o ESP-IDF põe o rádio em modem-sleep por
padrão (o firmware não muda isso): ele dorme entre os beacons do roteador e pode perder
heartbeats. Gasta menos (pasta 02), mas a coordenação nesse modo não foi testada.

As três placas estavam a centímetros umas das outras, no mesmo hub: a perda zero não vale para
paredes e distâncias de uma casa. Consumo sintético, sem sensor. Uma única rodada de 30 min.

## Reproduzir

```sh
cd firmware && cargo build --example espnow_teste
espflash flash --port COM3 --partition-table partitions.csv ../target/xtensa-esp32-espidf/debug/examples/espnow_teste
# idem COM7 e COM8; depois, no PowerShell:
& ok/parecer-revisor-2026-10-22/06-espnow-3-placas/captura.ps1     # ~31 min, reinicia as placas
python ok/parecer-revisor-2026-10-22/06-espnow-3-placas/analise.py
```

Os MACs dos papéis estão fixos no `espnow_teste.rs` (`PAPEIS`); outra placa precisa entrar lá.

## Remedição de 2026-10-24

Mesmas placas, mesmo `espnow_teste` (o fonte não mudou depois de 22/10; as placas não foram
regravadas). Saída em `remedicao-2026-10-24/`. Os arquivos desta pasta continuam sendo os de 22/10.

- Decisão: mediana de 190 ms, máximo de 1,24 s, igual a 22/10.
- Nenhum quadro perdido nos enlaces que chegaram ao fim.
- Teto: 55 s acima em 5 rajadas, a maior de 15 s, pico de +1.360 W (em 22/10: 69 s, a maior de 20 s).
- Quem saiu: sempre a air fryer, 136 s de uso negado; ferro e micro-ondas nunca cortados.
- Falha do ferro: as outras duas passam a contá-lo como 1.000 W fixos a partir de ~1.230 s e
  voltam a vê-lo em ~1.301 s, como em 22/10.
- A serial da COM8 (micro-ondas) parou aos 1.752 s de uptime, sem o resumo final. A placa seguiu
  transmitindo (as outras continuaram recebendo dela sem perda): falha do USB, não do rádio. No
  mesmo dia o `espflash` não conseguiu pôr nenhuma das três em modo de gravação (`WrongBootMode
  0x13`), o que aponta para o hub.
- O `captura.ps1` não zerou os `COM*.txt` antes de gravar: a captura nova entrou no fim dos de
  22/10 e foi separada pelo carimbo do PC.

Segunda remedição no mesmo dia (`remedicao-2026-10-24-b/`), depois de regravar o `espnow_teste`
nas COM7 e COM8 (que tinham ido para a bancada): decisão com mediana de 190 ms e máximo de 1,25 s;
nenhum quadro perdido; 51 s acima do teto em 5 rajadas, a maior de 11 s, pico de +1.360 W; sempre a
air fryer cortada (168 s); ferro mudo contado como 1.000 W fixos a partir de ~1.233 s. Desta vez
foi a serial da COM7 que parou antes do resumo final. O `Remove-Item` posto no `captura.ps1`
também não apagou o arquivo (a captura entrou de novo no fim do anterior e foi separada da mesma
forma): o PowerShell desta máquina acrescenta mas não apaga ou zera arquivos ali. Para outra
captura, apagar os `COM*.txt` antes, à mão ou pelo bash.
