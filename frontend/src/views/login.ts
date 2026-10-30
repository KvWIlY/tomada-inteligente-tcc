// =============================================
// Tela de login do dono (auth propria do backend)
// =============================================
//
// A MARCA (raio branco sobre quadrado verde + "SmartHome") abre a tela, no lugar do
// antigo titulo "Entrar"/"Criar conta": e a mesma marca do cabecalho da barra lateral
// (`sidebar.ts`), entao a primeira tela ja mostra a identidade que o app usa depois do
// login. Qual dos dois modos esta ativo continua legivel pelo botao de acao e pelo link
// do rodape, que trocam de texto juntos.
//
// Marca e subtitulo ficam CENTRALIZADOS; o formulario fica alinhado a ESQUERDA. Rotulo
// centralizado sobre campo de largura total solta o texto do campo que ele nomeia.
// O fluxo de autenticacao nao foi tocado.

import { icon } from '../icons';
import { escapeHtml } from './components';

export interface LoginProps {
  mode: 'login' | 'signup';
  email: string;
  senha: string;
  busy: boolean;
  error: string | null;
  info: string | null;
  onInput: (campo: 'email' | 'senha', valor: string) => void;
  onSubmit: () => void;
  onToggleMode: () => void;
}

export function renderLogin(el: HTMLElement, p: LoginProps): void {
  const acao = p.mode === 'login' ? 'Entrar' : 'Cadastrar';
  const alt = p.mode === 'login' ? 'Não tem conta? Criar uma' : 'Já tem conta? Entrar';

  const msg = p.error
    ? `<div class="alert" data-variant="destructive" role="alert">${icon('warn', 15)}<section><p>${escapeHtml(p.error)}</p></section></div>`
    : p.info
      ? `<div class="alert border-green bg-green-bg text-green-ink" role="status">${icon('check', 15)}<section><p>${escapeHtml(p.info)}</p></section></div>`
      : '';

  el.innerHTML = `
    <div class="grid min-h-full place-items-center p-8">
      <div class="card w-full max-w-[380px] text-center">
        <section>
          <div class="flex items-center justify-center gap-3">
            <span class="grid size-10 shrink-0 place-items-center rounded-md bg-green text-primary-foreground">${icon('bolt', 20)}</span>
            <span class="text-2xl leading-none font-bold tracking-tight">SmartHome</span>
          </div>
          <p class="mt-3 mb-6 text-sm font-medium text-ink-soft">Acesse o painel da sua casa com sua conta.</p>

          <form class="fieldset text-left" novalidate>
            <div class="field">
              <label class="label" for="email">E-mail</label>
              <input class="input" id="email" type="email" name="email" autocomplete="email"
                value="${escapeHtml(p.email)}" placeholder="voce@email.com" ${p.busy ? 'disabled' : ''}/>
            </div>
            <div class="field">
              <label class="label" for="senha">Senha</label>
              <input class="input" id="senha" type="password" name="senha" autocomplete="${p.mode === 'login' ? 'current-password' : 'new-password'}"
                value="${escapeHtml(p.senha)}" placeholder="••••••••" ${p.busy ? 'disabled' : ''}/>
            </div>
            ${msg}
            <button class="btn w-full" type="submit" ${p.busy ? 'disabled' : ''}>${p.busy ? 'Aguarde…' : acao}</button>
          </form>
        </section>
        <footer class="justify-center">
          <button class="btn" data-variant="link" type="button" data-role="alt">${alt}</button>
        </footer>
      </div>
    </div>`;

  const form = el.querySelector<HTMLFormElement>('form')!;
  const emailEl = el.querySelector<HTMLInputElement>('input[name="email"]')!;
  const senhaEl = el.querySelector<HTMLInputElement>('input[name="senha"]')!;

  emailEl.addEventListener('input', () => p.onInput('email', emailEl.value));
  senhaEl.addEventListener('input', () => p.onInput('senha', senhaEl.value));
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    p.onSubmit();
  });
  el.querySelector('[data-role="alt"]')?.addEventListener('click', () => p.onToggleMode());
}
