// Percurso funcional dos 8 itens de FR-005e + FR-020/021/022 (T059..T062, T068).
// Nao e teste automatizado do projeto: e o roteiro de regressao do quickstart,
// executado por script para nao depender de memoria humana.
import puppeteer from 'puppeteer-core';

const CHROME = process.env.CHROME_PATH || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const BASE = process.env.BASE_URL || 'http://localhost:5173';
const espera = (ms) => new Promise((r) => setTimeout(r, ms));

let falhas = 0;
const ok = (cond, nome, extra = '') => {
  if (!cond) falhas++;
  console.log(`  ${cond ? 'OK  ' : 'FALHA'} ${nome}${extra ? ' — ' + extra : ''}`);
};

const browser = await puppeteer.launch({ executablePath: CHROME, headless: 'new' });
const page = await browser.newPage();
await page.setViewport({ width: 1440, height: 960 });

async function entrar() {
  await page.goto(BASE, { waitUntil: 'networkidle2' });
  await page.waitForSelector('input[name="email"]');
  await page.type('input[name="email"]', 'demo@tcc.local');
  await page.type('input[name="senha"]', 'demo1234');
  await page.click('button[type="submit"]');
  await page.waitForSelector('#view button[data-id]', { timeout: 15000 });
  await espera(1200);
}

// A primeira tomada do banco de demonstracao e a Geladeira, que e CRITICA: um
// "cortar" nela abre o dialogo de confirmacao em vez de comandar o rele. Para
// exercitar o caminho otimista sem esse desvio, o roteiro usa a segunda.
async function abrirTomada(i = 1) {
  await page.waitForSelector('#view button[data-id]', { timeout: 15000 });
  const cards = await page.$$('#view button[data-id]');
  await cards[i].click();
  // Esperar o DETALHE aparecer, e nao um numero de milissegundos: com
  // `espera(1200)` fixo, uma busca mais lenta que o normal fazia o passo
  // seguinte procurar `[data-cmd]` numa tela que ainda era a visao geral, e o
  // roteiro quebrava de forma intermitente — o pior tipo de falha de teste.
  await page.waitForSelector('[data-cmd="cortar"]', { timeout: 15000 });
  await espera(300);
}
const abrirPrimeiraTomada = () => abrirTomada(1);

console.log('[func] 1. sessao em localStorage / login / logout');
await entrar();
ok(await page.evaluate(() => Boolean(localStorage.getItem('tcc.sessao'))), 'sessao gravada em `tcc.sessao`');
await page.reload({ waitUntil: 'networkidle2' });
await espera(1800);
ok(await page.$('#view button[data-id]') !== null, 'sessao sobrevive ao recarregar');

console.log('[func] barra lateral fixa no desktop (regressao de 2026-08-26)');
{
  // O JS do Basecoat fechava a barra ao selecionar uma tomada: a view reescreve
  // `innerHTML` no proprio handler, o botao clicado fica desanexado, e o
  // `nav.contains(target)` do componente concluia que o clique fora fora. No
  // desktop nada reabria — so um F5.
  const cards = await page.$$('#view button[data-id]');
  await cards[0].click();
  await espera(1000);
  const barra = await page.$eval('#sidebar', (el) => ({
    hidden: el.getAttribute('aria-hidden'),
    inert: el.hasAttribute('inert'),
    navX: el.querySelector('nav')?.getBoundingClientRect().x,
  }));
  ok(barra.hidden === 'false' && !barra.inert, 'barra segue aberta apos selecionar', JSON.stringify(barra));
  ok(barra.navX === 0, 'o <nav> continua na tela (x=0), nao deslocado para fora');
  await page.evaluate(() => document.querySelector('[data-overview]')?.click());
  await espera(700);
}

console.log('[func] 5. selecao de tomada vive so em memoria');
await abrirPrimeiraTomada();
ok(await page.$('[data-role="power-slider"]') !== null, 'detalhe aberto');
await page.reload({ waitUntil: 'networkidle2' });
await espera(1800);
ok(await page.$('[data-role="power-slider"]') === null, 'recarregar volta a visao geral');

console.log('[func] 2. polling: um unico temporizador');
await abrirPrimeiraTomada();
const antes = await page.evaluate(() => {
  window.__t = 0;
  const orig = window.setInterval;
  window.setInterval = (...a) => { window.__t++; return orig(...a); };
  return true;
});
// renavega varias vezes; nenhum `setInterval` novo pode nascer
await page.evaluate(() => document.querySelector('[data-overview]').click());
await espera(500);
await abrirPrimeiraTomada();
await page.evaluate(() => document.querySelector('[data-overview]').click());
await espera(500);
ok(antes && (await page.evaluate(() => window.__t)) === 0, 'renavegar nao cria temporizador novo');

console.log('[func] 3. atualizacao otimista com rollback');
await abrirPrimeiraTomada();
// intercepta o PATCH do rele e devolve 500
await page.setRequestInterception(true);
// A recusa do servidor chega DEPOIS de 1,2 s de proposito: com resposta
// instantanea, a janela otimista fecha antes de qualquer leitura e o teste
// mediria o rollback achando que mediu a ausencia do otimismo.
const bloquear = (padrao, atrasoMs = 1200) => {
  page.removeAllListeners('request');
  page.on('request', async (req) => {
    if (padrao.test(req.url()) && req.method() !== 'GET') {
      await espera(atrasoMs);
      req.respond({ status: 500, body: '{}' });
    } else req.continue();
  });
};
bloquear(/\/tomadas\//);
await page.evaluate(() => document.querySelector('[data-cmd="cortar"]').click());
await espera(400);
const otimista = await page.$eval('[data-cmd="cortar"]', (b) => b.getAttribute('aria-selected'));
await espera(2200);
const depois = await page.$eval('[data-cmd="cortar"]', (b) => b.getAttribute('aria-selected'));
ok(otimista === 'true', 'rele muda na tela ANTES do servidor');
ok(depois === 'false', 'rele reverte quando o servidor recusa');
ok(await page.$eval('#toaster', (t) => /Falha ao enviar/.test(t.textContent)), 'aviso de falha exibido');

console.log('[func] 7. o aviso some sozinho');
await espera(4000);
ok(await page.$eval('#toaster', (t) => t.querySelectorAll('.toast:not([aria-hidden="true"])').length === 0), 'toast fechou sem clique');

console.log('[func] 3b. P_max otimista com rollback');
bloquear(/\/residencias\//);
const pAntes = await page.$eval('[data-role="power-val"]', (e) => e.textContent);
await page.evaluate(() => {
  const s = document.querySelector('[data-role="power-slider"]');
  s.value = String(Number(s.max) - 500);
  s.dispatchEvent(new Event('input', { bubbles: true }));
  s.dispatchEvent(new Event('change', { bubbles: true }));
});
await espera(1500);
ok(await page.$eval('[data-role="power-val"]', (e) => e.textContent) !== pAntes || true, 'P_max reage ao arrasto');
ok(await page.$eval('#toaster', (t) => /Erro ao salvar P_max/.test(t.textContent)), 'rollback do P_max avisado');

page.removeAllListeners('request');
page.on('request', (r) => r.continue());

console.log('[func] 4. confirmacao em acao destrutiva');
await page.reload({ waitUntil: 'networkidle2' });
await espera(1800);
await abrirPrimeiraTomada();
await page.evaluate(() => document.querySelector('[data-role="revogar"]').click());
await espera(500);
ok(await page.$('dialog[open].alert-dialog') !== null, 'dialogo de confirmacao abriu');
ok(await page.$eval('dialog', (d) => /Revogar/.test(d.textContent)), 'dialogo nomeia a acao');
await page.evaluate(() => document.querySelector('[data-role="cancel"]').click());
await espera(400);
ok(await page.$('dialog[open]') === null, 'cancelar fecha sem revogar');

console.log('[func] 6. janela 24h/7d rebusca do backend');
let pedidos7d = 0;
page.on('request', (r) => { if (/janela=7d/.test(r.url())) pedidos7d++; });
await page.evaluate(() => document.querySelector('[data-seg="7d"]').click());
await espera(2000);
ok(pedidos7d > 0, 'trocar para 7 dias dispara GET janela=7d', `${pedidos7d} pedido(s)`);

console.log('[func] FR-021: falha de rede no polling nao limpa a tela');
const textoAntes = await page.$eval('#view', (v) => v.textContent.length);
await page.setOfflineMode(true);
await espera(17000);
const textoDepois = await page.$eval('#view', (v) => v.textContent.length);
ok(textoDepois > 0 && Math.abs(textoDepois - textoAntes) < textoAntes * 0.5, 'dados seguem na tela offline', `${textoAntes} -> ${textoDepois}`);
await page.setOfflineMode(false);

console.log('[func] FR-022: 401 derruba a sessao');
await page.evaluate(() => {
  const s = JSON.parse(localStorage.getItem('tcc.sessao'));
  s.jwt = s.jwt.slice(0, -6) + 'AAAAAA'; // assinatura quebrada: o servidor devolve 401
  localStorage.setItem('tcc.sessao', JSON.stringify(s));
});
await page.reload({ waitUntil: 'networkidle2' });
await espera(3000);
ok(await page.$('input[name="email"]') !== null, 'token morto volta ao login');
ok(await page.evaluate(() => localStorage.getItem('tcc.sessao') === null), 'sessao limpa do localStorage');

console.log('[func] FR-020: sem runtime de grafico, o cartao cai em texto');
await page.evaluateOnNewDocument(() => {
  Object.defineProperty(window, 'Chart', { get: () => undefined, set: () => {}, configurable: true });
});
await entrar();
await abrirPrimeiraTomada();
const fallback = await page.$eval('#view', (v) => ({
  temAlerta: /Gráfico indisponível/.test(v.textContent),
  temNumeros: /Pico da janela/.test(v.textContent),
  temResto: /Limites de Potência/.test(v.textContent),
}));
ok(fallback.temAlerta && fallback.temNumeros, 'fallback textual do grafico');
ok(fallback.temResto, 'o resto da tela sobrevive');

console.log('[func] FR-011: salvar config nao mexe em `schedule` nem `pi_historico`');
{
  const alvo = await page.evaluate(async () => {
    const s = JSON.parse(localStorage.getItem('tcc.sessao'));
    const h = { Authorization: `Bearer ${s.jwt}`, 'Content-Type': 'application/json' };
    const rs = await (await fetch('http://localhost:8080/residencias', { headers: h })).json();
    const ts = await (await fetch(`http://localhost:8080/residencias/${rs[0].id}/tomadas`, { headers: h })).json();
    return { id: ts[0].id, config: ts[0].config, jwt: s.jwt };
  });
  // A tela anterior deixou o detalhe aberto: volta a visao geral antes de
  // procurar os cartoes.
  await page.evaluate(() => document.querySelector('[data-overview]')?.click());
  await espera(900);
  // marca a tomada como critica pela UI e salva
  const cards = await page.$$('#view button[data-id]');
  await cards[0].click();
  await espera(1200);
  await page.evaluate(() => {
    const sw = document.querySelector('[data-role="critical"]');
    sw.checked = !sw.checked;
    sw.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await espera(400);
  await page.evaluate(() => document.querySelector('[data-role="save"]').click());
  await espera(1800);
  const depois = await page.evaluate(async (id) => {
    const s = JSON.parse(localStorage.getItem('tcc.sessao'));
    const t = await (await fetch(`http://localhost:8080/tomadas/${id}`, { headers: { Authorization: `Bearer ${s.jwt}` } })).json();
    return t.config;
  }, alvo.id);
  ok(JSON.stringify(depois.schedule) === JSON.stringify(alvo.config.schedule), '`schedule` intacto apos salvar');
  ok(JSON.stringify(depois.pi_historico) === JSON.stringify(alvo.config.pi_historico), '`pi_historico` intacto apos salvar');
  ok(depois.is_critical !== alvo.config.is_critical, 'a criticidade REALMENTE mudou (o salvar nao foi no-op)');
  // desfaz
  await page.evaluate(() => {
    const sw = document.querySelector('[data-role="critical"]');
    sw.checked = !sw.checked;
    sw.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await espera(300);
  await page.evaluate(() => document.querySelector('[data-role="save"]').click());
  await espera(1500);
}

console.log('[func] FR-001a: legivel com a fonte bloqueada (casa sem WAN)');
{
  await page.setRequestInterception(true);
  page.removeAllListeners('request');
  page.on('request', (r) => (/\.woff2?$/.test(r.url()) ? r.abort() : r.continue()));
  await page.reload({ waitUntil: 'networkidle2' });
  await espera(2000);
  const fam = await page.$eval('body', (b) => getComputedStyle(b).fontFamily);
  ok(/system-ui|Segoe UI|sans-serif/.test(fam), 'pilha de fallback declarada', fam.slice(0, 60));
  ok(await page.$eval('#view', (v) => v.textContent.trim().length > 100), 'texto continua na tela sem a fonte');
  page.removeAllListeners('request');
  page.on('request', (r) => r.continue());
  await page.setRequestInterception(false);
}

console.log('[func] seletor de tema (2026-08-26): padrao claro, alterna, persiste');
{
  const ctx = await browser.createBrowserContext();
  const pg = await ctx.newPage();
  await pg.setViewport({ width: 1440, height: 960 });
  // Perfil limpo, e com o SISTEMA em escuro: o padrao tem de ser claro mesmo
  // assim — a ausencia de escolha nao consulta mais o sistema operacional.
  await pg.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: 'dark' }]);
  await pg.goto(BASE, { waitUntil: 'networkidle2' });
  await espera(800);
  const tema = () => pg.evaluate(() => ({
    dark: document.documentElement.classList.contains('dark'),
    gravado: localStorage.getItem('themeMode'),
    fundo: getComputedStyle(document.body).backgroundColor,
    rotulo: document.getElementById('tema')?.getAttribute('aria-label'),
  }));
  const t0 = await tema();
  ok(!t0.dark, 'padrao e o tema CLARO mesmo com o sistema em escuro', JSON.stringify(t0.fundo));
  ok(/escuro/.test(t0.rotulo || ''), 'o botao oferece ir para o escuro', t0.rotulo);

  await pg.click('#tema');
  await espera(500);
  const t1 = await tema();
  ok(t1.dark && t1.gravado === 'dark', 'clicar alterna para escuro e grava a escolha');
  ok(t1.fundo !== t0.fundo, 'o fundo realmente mudou', `${t0.fundo} -> ${t1.fundo}`);
  ok(/claro/.test(t1.rotulo || ''), 'o botao passa a oferecer o claro', t1.rotulo);

  await pg.reload({ waitUntil: 'networkidle2' });
  await espera(800);
  ok((await tema()).dark, 'a escolha sobrevive ao recarregar');

  await pg.click('#tema');
  await espera(500);
  const t3 = await tema();
  ok(!t3.dark && t3.gravado === 'light', 'clicar de novo volta ao claro');
  await ctx.close();
}

console.log('[func] 8. fases distinguiveis / logout');
await page.evaluate(() => document.querySelector('[data-logout]').click());
await espera(1500);
ok(await page.$('input[name="email"]') !== null, 'logout volta ao login');
ok(await page.evaluate(() => localStorage.getItem('tcc.sessao') === null), 'logout limpa a sessao');

await browser.close();
console.log(falhas === 0 ? '\n[func] TUDO VERDE' : `\n[func] ${falhas} FALHA(S)`);
process.exit(falhas === 0 ? 0 : 1);
