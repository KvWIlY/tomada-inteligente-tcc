// Captura headless das 7 telas da feature 007.
//   node shots.mjs <dir-de-saida> [light|dark]
// Requer: backend em :8080 e o frontend servido em :5173.
import puppeteer from 'puppeteer-core';
import { mkdirSync } from 'node:fs';

const CHROME = process.env.CHROME_PATH || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const BASE = process.env.BASE_URL || 'http://localhost:5173';
const OUT = process.argv[2] || './shots';
const TEMA = process.argv[3] === 'dark' ? 'dark' : 'light';

mkdirSync(OUT, { recursive: true });

const espera = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: 'new',
  args: ['--force-device-scale-factor=1', '--hide-scrollbars'],
});
const page = await browser.newPage();
page.on('console', (m) => { if (m.type() === 'error') console.log('  [console]', m.text()); });
page.on('pageerror', (e) => console.log('  [pageerror]', e.message));
// O tema deixou de seguir o sistema em 2026-08-26: agora e uma classe no
// `<html>`, escolhida por botao e persistida em `localStorage` sob `themeMode`.
// Emular `prefers-color-scheme` aqui nao trocaria nada — o roteiro de escuro
// passaria a fotografar o tema claro sem reclamar.
await page.evaluateOnNewDocument((modo) => {
  try { localStorage.setItem('themeMode', modo); } catch (_) { /* ignora */ }
}, TEMA);
await page.setViewport({ width: 1440, height: 960 });

/**
 * Captura a pagina inteira SEM usar `fullPage`.
 *
 * O `fullPage` do Chrome redimensiona a viewport para a altura do conteudo e
 * fotografa em seguida — e o Chart.js, que e `responsive`, limpa o canvas e so
 * redesenha no quadro seguinte. O resultado era um cartao de grafico VAZIO na
 * captura, com o grafico perfeitamente visivel no navegador. Custou uma sessao
 * de caca a um defeito que nao existia.
 *
 * Aqui a viewport e esticada ANTES, com tempo para os graficos assentarem, e a
 * captura sai da viewport mesma.
 */
async function tira(nome, alvo = page) {
  await espera(700);
  const largura = page.viewport().width;
  const altura = await page.evaluate(() => Math.ceil(document.documentElement.scrollHeight));
  await page.setViewport({ width: largura, height: altura });
  await espera(500); // o Chart.js redesenha no quadro seguinte ao resize
  await alvo.screenshot({ path: `${OUT}/${nome}.png` });
  await page.setViewport({ width: largura, height: 960 });
  console.log('  ->', nome);
}

console.log(`[shots] ${TEMA} -> ${OUT}`);
await page.goto(BASE, { waitUntil: 'networkidle2' });

// 1. login — o roteiro loga pela UI real, entao esta captura tambem prova FR-008.
await page.waitForSelector('input[name="email"]');
await tira('01-login');

await page.type('input[name="email"]', 'demo@tcc.local');
await page.type('input[name="senha"]', 'demo1234');
await Promise.all([page.click('button[type="submit"]'), espera(2500)]);

// 2. visao geral
await page.waitForSelector('#view button[data-id]', { timeout: 15000 });
await tira('02-visao-geral');

// 3..5. um detalhe por tomada. A app reescreve `innerHTML`, entao os handles
// ficam detached: e preciso re-consultar os cartoes a cada volta do laco.
const nomes = await page.$$eval('#view button[data-id]', (bs) => bs.map((b) => b.textContent.trim().split('\n')[0].trim()));
for (let i = 0; i < nomes.length; i++) {
  const cards = await page.$$('#view button[data-id]');
  await cards[i].click();
  await espera(1200);
  const slug = nomes[i].toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '').replace(/[^a-z0-9]+/g, '-');
  await tira(`0${3 + i}-detalhe-${slug}`);

  // 3b. o seletor de 7 dias, so na primeira tomada
  if (i === 0) {
    const achou7d = await page.evaluate(() => {
      const b = document.querySelector('[data-seg="7d"]');
      if (b) b.click();
      return Boolean(b);
    });
    console.log('  [7d]', achou7d ? 'clicado' : 'SELETOR NAO ENCONTRADO');
    await espera(2000);
    await tira('03b-detalhe-7dias');
    await page.evaluate(() => document.querySelector('[data-seg="24h"]')?.click());
    await espera(900);
  }
  await page.evaluate(() => document.querySelector('[data-overview]').click());
  await espera(900);
}

// 7. celular (412 px, FR-014/SC-007)
await page.setViewport({ width: 412, height: 915 });
await espera(900);
await tira('07-celular-visao-geral');

await browser.close();
console.log('[shots] ok');
