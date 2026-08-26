// Verificacoes medidas (nao estimadas) de contraste e foco — FR-002c, FR-015.
//   node checks.mjs [light|dark]
import puppeteer from 'puppeteer-core';

const CHROME = process.env.CHROME_PATH || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const BASE = process.env.BASE_URL || 'http://localhost:5173';
const TEMA = process.argv[2] === 'dark' ? 'dark' : 'light';
const espera = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await puppeteer.launch({ executablePath: CHROME, headless: 'new' });
const page = await browser.newPage();
await page.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: TEMA }]);
await page.setViewport({ width: 1440, height: 960 });
await page.goto(BASE, { waitUntil: 'networkidle2' });

// ---- utilitarios injetados ----
await page.evaluateOnNewDocument(() => {});
const helpers = `
  const lum = (rgb) => {
    const [r, g, b] = rgb.map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const parse = (s) => (s.match(/[\\d.]+/g) || []).slice(0, 3).map(Number);
  const fundoDe = (el) => {
    let n = el;
    while (n) {
      const bg = getComputedStyle(n).backgroundColor;
      const p = parse(bg);
      if (bg && !bg.includes('rgba(0, 0, 0, 0)') && p.length === 3) return p;
      n = n.parentElement;
    }
    return [255, 255, 255];
  };
  const razao = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
`;

async function contraste(rotulo, seletor, propriedade = 'color') {
  const r = await page.evaluate(
    // eslint-disable-next-line no-new-func
    new Function('sel', 'prop', `${helpers}
      const el = document.querySelector(sel);
      if (!el) return null;
      const cs = getComputedStyle(el);
      const frente = prop === 'borderColor' ? parse(cs.borderTopColor) : parse(cs[prop]);
      const fundo = fundoDe(prop === 'borderColor' ? el.parentElement : el);
      return { valor: prop === 'borderColor' ? cs.borderTopColor : cs[prop], razao: razao(frente, fundo) };
    `),
    seletor,
    propriedade,
  );
  console.log(r ? `  ${rotulo}: ${r.valor} -> ${r.razao.toFixed(2)}:1` : `  ${rotulo}: NAO ENCONTRADO (${seletor})`);
  return r;
}

console.log(`[checks] tema ${TEMA}`);
console.log(' login');
await page.waitForSelector('input[name="email"]');
await contraste('borda do campo (FR-002c / WCAG 1.4.11 >= 3)', 'input[name="email"]', 'borderColor');
const foco = await page.evaluate(() => {
  const el = document.querySelector('input[name="email"]');
  el.focus();
  const cs = getComputedStyle(el);
  return { outline: cs.outlineWidth + ' ' + cs.outlineColor, ring: cs.boxShadow };
});
console.log('  foco do campo:', JSON.stringify(foco));

await page.type('input[name="email"]', 'demo@tcc.local');
await page.type('input[name="senha"]', 'demo1234');
await page.click('button[type="submit"]');
await page.waitForSelector('#view button[data-id]', { timeout: 15000 });
await espera(1200);

console.log(' visao geral');
await contraste('badge Standby (texto)', '.badge--STD');
await contraste('FP baixo no cartao', '#view .text-amber-ink');

// detalhe da tomada com FP 0,39
const idx = await page.$$eval('#view button[data-id]', (bs) => bs.findIndex((b) => /Carregadores/.test(b.textContent)));
const cards = await page.$$('#view button[data-id]');
await cards[idx].click();
await espera(1200);
console.log(' detalhe (Carregadores, FP 0,39)');
await contraste('fator de potencia (texto)', '#view .text-amber-ink');
console.log('  P_max slider:', JSON.stringify(await page.$eval('[data-role="power-slider"]', (i) => ({ value: i.value, max: i.max, pct: i.style.getPropertyValue('--rng-pct') }))));

// foco visivel em todo controle interativo (FR-015).
// Tem de ser TAB de verdade: `el.focus()` programatico nao casa `:focus-visible`
// em botao, e o teste daria falso negativo em quase tudo.
await page.evaluate(() => document.body.focus());
const vistos = new Set();
const semFoco = [];
let total = 0;
for (let i = 0; i < 60; i++) {
  await page.keyboard.press('Tab');
  const r = await page.evaluate(() => {
    const el = document.activeElement;
    if (!el || el === document.body) return null;
    const cs = getComputedStyle(el);
    const temOutline = cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) > 0;
    const temRing = cs.boxShadow !== 'none' && !/^rgba\(0, 0, 0, 0\) 0px 0px 0px 0px$/.test(cs.boxShadow);
    const temBorda = cs.borderTopColor !== 'rgba(0, 0, 0, 0)';
    const id = (el.tagName + ':' + (el.dataset.role || el.dataset.cmd || el.dataset.seg || el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 40));
    return { id, ok: temOutline || temRing, temBorda };
  });
  if (!r) continue;
  if (vistos.has(r.id)) continue;
  vistos.add(r.id);
  total++;
  if (!r.ok) semFoco.push(r.id);
}
console.log(` foco visivel ao Tab: ${total - semFoco.length}/${total}`, semFoco.length ? semFoco : '');

await browser.close();
