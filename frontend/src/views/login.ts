// =============================================
// Tela de login do dono (auth propria do backend)
// =============================================
//
// FR-006: o emblema de raio sobre quadrado verde saiu. Ele repetia, em forma de
// selo, o que o titulo ja diz — e era o unico lugar da interface com um bloco
// de cor solida desse tamanho.
// FR-007: titulo, subtitulo, rotulos e mensagens ficam centralizados.
// FR-008: o fluxo de autenticacao nao foi tocado.

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
  const titulo = p.mode === 'login' ? 'Entrar' : 'Criar conta';
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
        <header class="items-center">
          <div class="text-lg font-bold tracking-tight">SmartHome</div>
          <p class="text-xs font-medium text-ink-faint">Gerenciador de Energia</p>
        </header>
        <section>
          <h1 class="text-2xl font-bold tracking-tight">${titulo}</h1>
          <p class="mt-1.5 mb-6 text-sm font-medium text-ink-soft">Acesse o painel da sua casa com sua conta.</p>

          <form class="fieldset [&_.field]:items-center" novalidate>
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
