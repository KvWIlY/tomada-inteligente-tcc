# Verificação do dashboard

Não existe teste unitário de frontend neste projeto. O que existe é isto: três roteiros
headless que dirigem a interface real contra o backend real. Eles nasceram na feature 007,
depois de três defeitos visuais passarem por `tsc && vite build` sem um pio — **build verde não
prova nada sobre a tela**.

Ficam versionados aqui, e não num diretório temporário, porque a versão anterior deles foi
perdida exatamente por viver fora do repositório.

## Pré-requisitos

```powershell
cd C:\y\backend  ; cargo run     # :8080 — aplica as migrations no boot
cd C:\y\frontend ; npm run build ; npm run preview   # :5173 (ou `npm run dev`)
```

Login do banco de demonstração: `demo@tcc.local` / `demo1234`.

O Chrome é localizado em `C:/Program Files/Google/Chrome/Application/chrome.exe`; outro caminho
entra por `CHROME_PATH`. A URL da aplicação entra por `BASE_URL`.

> As medições do banco de demonstração são de uma janela de 24 h fixa. Se elas envelhecerem, a
> tela passa a mostrar "sem dados ainda" — corretamente. Para deslocar o dia inteiro para
> agora:
>
> ```sql
> SET @d = (SELECT UNIX_TIMESTAMP() - MAX(timestamp) - 120 FROM medicoes);
> UPDATE medicoes SET timestamp = timestamp + @d;
> UPDATE eventos  SET timestamp = timestamp + @d;
> ```

## Os três roteiros

| Comando | O que faz | Quando rodar |
|---|---|---|
| `npm run verify` | Percorre os **8 itens de FR-005e** mais FR-020/021/022: sessão, polling, atualização otimista com rollback, confirmação destrutiva, seleção em memória, rebusca da janela, sumiço do aviso, fases. Sai com código ≠ 0 se algo falhar. | Antes de qualquer commit que toque `main.ts` ou as views |
| `npm run checks -- light` / `-- dark` | **Mede** contraste (WCAG AA) e foco visível ao Tab. Imprime a razão de cada par — número medido, não estimado. | Ao mexer em token de cor |
| `npm run shots -- <dir> light\|dark` | Captura as 7 telas do quickstart em página inteira. | A cada etapa visual, comparando com o conjunto anterior |

`verify` derruba a rede, quebra a assinatura do JWT e faz o servidor recusar comandos de propósito
— é o único jeito de provar que o rollback e os estados de erro funcionam. Ele **não** revoga
dispositivo: abre o diálogo e cancela.

## O que já custou uma iteração

- A aplicação reescreve `innerHTML` a cada render, então handles de elemento ficam *detached*.
  Re-consulte o seletor a cada volta do laço.
- `#view button[data-id]` e `#sidebar button[data-id]` casam os dois lados. Escope pelo `#view`.
- A primeira tomada do banco é a Geladeira, que é **crítica**: "cortar" nela abre confirmação em
  vez de comandar o relé.
- `el.focus()` programático não casa `:focus-visible` num `<button>`. Auditoria de foco só vale
  com `Tab` de verdade.
- Uma recusa instantânea do servidor fecha a janela otimista antes de qualquer leitura: o
  `verify` atrasa a resposta de propósito para conseguir observá-la.
