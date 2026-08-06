// =============================================
// Tela de login do dono (auth propria do backend)
// =============================================

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
    ? `<div class="auth__msg auth__msg--err">${icon('warn', 15, 2.2)}<span>${escapeHtml(p.error)}</span></div>`
    : p.info
      ? `<div class="auth__msg auth__msg--ok">${icon('check', 15, 2.6)}<span>${escapeHtml(p.info)}</span></div>`
      : '';

  el.innerHTML = `
    <div class="auth">
      <div class="auth__card">
        <div class="auth__brand">
          <span class="brand__mark">${icon('bolt', 22)}</span>
          <div>
            <div class="brand__name">SmartHome</div>
            <div class="brand__sub">Gerenciador de Energia</div>
          </div>
        </div>
        <h1 class="auth__title">${titulo}</h1>
        <p class="auth__sub">Acesse o painel da sua casa com sua conta.</p>

        <form class="auth__form" novalidate>
          <label class="auth__label">E-mail
            <input class="auth__input" type="email" name="email" autocomplete="email"
              value="${escapeHtml(p.email)}" placeholder="voce@email.com" ${p.busy ? 'disabled' : ''}/>
          </label>
          <label class="auth__label">Senha
            <input class="auth__input" type="password" name="senha" autocomplete="${p.mode === 'login' ? 'current-password' : 'new-password'}"
              value="${escapeHtml(p.senha)}" placeholder="••••••••" ${p.busy ? 'disabled' : ''}/>
          </label>
          ${msg}
          <button class="btn btn--green auth__submit" type="submit" ${p.busy ? 'disabled' : ''}>${p.busy ? 'Aguarde…' : acao}</button>
        </form>

        <button class="auth__alt" type="button">${alt}</button>
      </div>
    </div>`;

  const form = el.querySelector<HTMLFormElement>('.auth__form')!;
  const emailEl = el.querySelector<HTMLInputElement>('input[name="email"]')!;
  const senhaEl = el.querySelector<HTMLInputElement>('input[name="senha"]')!;

  emailEl.addEventListener('input', () => p.onInput('email', emailEl.value));
  senhaEl.addEventListener('input', () => p.onInput('senha', senhaEl.value));
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    p.onSubmit();
  });
  el.querySelector('.auth__alt')?.addEventListener('click', () => p.onToggleMode());
}
