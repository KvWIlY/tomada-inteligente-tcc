//! Base CSV de consumo (data-model §1): validacao e leitura por retencao de ordem zero
//! (research D3, D7).
//!
//! A base NAO fica em memoria. `carregar` percorre o arquivo uma vez, em fluxo, para validar e
//! calcular o SHA-256; o laco le com um `Leitor`, que guarda so a linha corrente e avanca
//! junto com o passo. Carregada inteira, uma base de 30 d a cada 10 s com 250 colunas passava
//! de 380 MB (o arquivo mais os numeros); em fluxo, a memoria e a de uma linha.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};

use crate::caso::{invalido, Caso};
use crate::firmware::hex;

/// Buffer de leitura: cabe uma linha de milhares de colunas sem ir ao disco por campo.
const BUFFER: usize = 1 << 16;

pub struct Base {
    pub arquivo: PathBuf,
    pub passo_s: i64,
    pub linhas: usize,
    pub sha256: String,
    /// Colunas de aparelho na ordem do cabecalho, sem o `t_s`.
    colunas: Vec<String>,
    /// Indice da coluna de cada tomada do caso, na ordem do caso.
    pub indices: Vec<usize>,
    pub avisos: Vec<String>,
}

impl Base {
    /// Leitor posicionado antes da primeira linha de dados.
    pub fn leitor(&self) -> Result<Leitor> {
        let rotulo = self.arquivo.display().to_string();
        let entrada = abrir_dados(&self.arquivo).map_err(|e| anyhow!("base {rotulo}: não consegui reabrir ({e})"))?;
        Ok(Leitor {
            rotulo,
            arquivo: self.arquivo.clone(),
            passo_s: self.passo_s,
            linhas: self.linhas,
            colunas: self.colunas.clone(),
            entrada,
            buf: Vec::with_capacity(BUFFER),
            num_linha: 1,
            atual: None,
            valores: Vec::with_capacity(self.colunas.len()),
        })
    }
}

/// A linha corrente da base e o arquivo aberto logo depois dela.
pub struct Leitor {
    rotulo: String,
    arquivo: PathBuf,
    passo_s: i64,
    linhas: usize,
    colunas: Vec<String>,
    entrada: BufReader<File>,
    buf: Vec<u8>,
    /// Linha do arquivo (1 = cabecalho) lida por ultimo, para as mensagens.
    num_linha: usize,
    /// Indice da linha de dados que esta em `valores`.
    atual: Option<usize>,
    valores: Vec<f32>,
}

impl Leitor {
    /// Carrega a linha que vale no instante: a ultima com `t_s` menor ou igual. O laco anda
    /// para a frente, entao e a mesma linha ou uma das seguintes; voltar reabre o arquivo.
    pub fn posicionar(&mut self, t_s: i64) -> Result<()> {
        let alvo = ((t_s.max(0) / self.passo_s) as usize).min(self.linhas - 1);
        if self.atual.is_some_and(|a| a > alvo) {
            self.entrada = abrir_dados(&self.arquivo).map_err(|e| self.falha(format!("não consegui reabrir ({e})")))?;
            self.num_linha = 1;
            self.atual = None;
        }
        while self.atual != Some(alvo) {
            self.avancar()?;
        }
        Ok(())
    }

    /// Valor da coluna na linha carregada por `posicionar`.
    #[inline]
    pub fn valor(&self, coluna: usize) -> f32 {
        self.valores[coluna]
    }

    fn avancar(&mut self) -> Result<()> {
        let esperado = self.atual.map_or(0, |a| a + 1);
        loop {
            let lido = proxima_crua(&mut self.entrada, &mut self.buf, None)
                .map_err(|e| self.falha(format!("erro de leitura ({e})")))?;
            if lido == 0 {
                return Err(self.falha(format!("acabou antes da linha de dados {esperado}")));
            }
            self.num_linha += 1;
            let linha = std::str::from_utf8(aparar(&self.buf)).map_err(|_| self.falha("o arquivo não é UTF-8".to_string()))?;
            let t_esperado = esperado as i64 * self.passo_s;
            match ler_linha(linha, self.num_linha, t_esperado, self.passo_s, &self.colunas, &mut self.valores) {
                Ok(true) => {
                    self.atual = Some(esperado);
                    return Ok(());
                }
                Ok(false) => continue,
                Err(m) => return Err(self.falha(m)),
            }
        }
    }

    fn falha(&self, msg: String) -> anyhow::Error {
        anyhow!("base {}: {msg}. O arquivo mudou depois de validado?", self.rotulo)
    }
}

fn abrir(caminho: &Path) -> std::io::Result<BufReader<File>> {
    Ok(BufReader::with_capacity(BUFFER, File::open(caminho)?))
}

/// Abre e consome o cabecalho, ja validado por `carregar`.
fn abrir_dados(caminho: &Path) -> std::io::Result<BufReader<File>> {
    let mut entrada = abrir(caminho)?;
    let mut cabecalho = Vec::new();
    entrada.read_until(b'\n', &mut cabecalho)?;
    Ok(entrada)
}

/// Proxima linha crua em `buf`, com o terminador, alimentando o hash se houver. 0 = fim.
fn proxima_crua(entrada: &mut BufReader<File>, buf: &mut Vec<u8>, hash: Option<&mut Sha256>) -> std::io::Result<usize> {
    buf.clear();
    let n = entrada.read_until(b'\n', buf)?;
    if let Some(h) = hash {
        h.update(&buf[..]);
    }
    Ok(n)
}

/// Tira o `\n` e o `\r` do fim, como `str::lines`.
fn aparar(buf: &[u8]) -> &[u8] {
    let b = buf.strip_suffix(b"\n").unwrap_or(buf);
    b.strip_suffix(b"\r").unwrap_or(b)
}

/// Valida uma linha de dados e deixa os valores em `valores`. `Ok(false)` e linha em branco,
/// que nao conta. As mensagens sao as mesmas da validacao que carregava tudo.
fn ler_linha(
    linha: &str,
    num_linha: usize,
    esperado: i64,
    passo: i64,
    colunas: &[String],
    valores: &mut Vec<f32>,
) -> std::result::Result<bool, String> {
    if linha.trim().is_empty() {
        return Ok(false);
    }
    let mut campos = linha.split(',');
    let t_txt = campos.next().unwrap_or_default().trim();
    let t: i64 = t_txt.parse().map_err(|_| format!("linha {num_linha} coluna t_s: \"{t_txt}\" não é inteiro"))?;
    if t != esperado {
        return Err(format!(
            "linha {num_linha}: t_s = {t}, era esperado {esperado} (a série começa em 0 e sobe de base.passo_s = {passo}, sem lacuna, repetição ou inversão)"
        ));
    }
    valores.clear();
    for campo in campos {
        let c = valores.len();
        if c >= colunas.len() {
            return Err(format!("linha {num_linha}: mais valores que colunas no cabeçalho ({})", colunas.len()));
        }
        let txt = campo.trim();
        let v: f32 = txt.parse().map_err(|_| format!("linha {num_linha} coluna {}: \"{txt}\" não é número", colunas[c]))?;
        if !v.is_finite() {
            return Err(format!("linha {num_linha} coluna {}: valor {txt} não é finito", colunas[c]));
        }
        if v < 0.0 {
            return Err(format!("linha {num_linha} coluna {}: valor negativo {txt}", colunas[c]));
        }
        valores.push(v);
    }
    if valores.len() != colunas.len() {
        return Err(format!("linha {num_linha}: {} valores, o cabeçalho tem {}", valores.len(), colunas.len()));
    }
    Ok(true)
}

/// Valida a base inteira em uma passada, sem guardar os valores.
pub fn carregar(caso: &Caso) -> Result<Base> {
    let caminho = &caso.base_arquivo;
    let rotulo = caminho.display().to_string();
    let recusa = |msg: String| invalido(format!("base {rotulo}: {msg}"));
    let mut entrada = abrir(caminho).map_err(|e| {
        let dica = if caso.tem_perfis() { "; o caso tem perfis, gere com `gerar-base`" } else { "" };
        invalido(format!("base {rotulo}: não consegui ler ({e}){dica}"))
    })?;
    let mut hash = Sha256::new();
    let mut buf = Vec::with_capacity(BUFFER);
    let erro_leitura = |e: std::io::Error| invalido(format!("base {rotulo}: não consegui ler ({e})"));

    let lido = proxima_crua(&mut entrada, &mut buf, Some(&mut hash)).map_err(erro_leitura)?;
    let colunas: Vec<String> = {
        let cabecalho = std::str::from_utf8(aparar(&buf)).map_err(|_| recusa("o arquivo não é UTF-8".to_string()))?;
        if lido == 0 || cabecalho.trim().is_empty() {
            return Err(recusa("arquivo vazio, sem cabeçalho".to_string()));
        }
        let mut cab = cabecalho.split(',').map(|c| c.trim().trim_start_matches('\u{feff}').to_string());
        let primeira = cab.next().unwrap_or_default();
        if primeira != "t_s" {
            return Err(recusa(format!("a primeira coluna do cabeçalho tem que ser t_s, veio \"{primeira}\"")));
        }
        cab.collect()
    };
    if colunas.is_empty() {
        return Err(recusa("cabeçalho sem nenhuma coluna de aparelho".to_string()));
    }
    let mut vistos = HashSet::new();
    for c in &colunas {
        if !vistos.insert(c.as_str()) {
            return Err(recusa(format!("coluna \"{c}\" repetida no cabeçalho")));
        }
    }

    let passo = caso.base_passo_s;
    let mut valores = Vec::with_capacity(colunas.len());
    let mut n = 0usize;
    let mut num_linha = 1usize;
    while proxima_crua(&mut entrada, &mut buf, Some(&mut hash)).map_err(erro_leitura)? > 0 {
        num_linha += 1;
        let linha = std::str::from_utf8(aparar(&buf)).map_err(|_| recusa("o arquivo não é UTF-8".to_string()))?;
        if ler_linha(linha, num_linha, n as i64 * passo, passo, &colunas, &mut valores).map_err(recusa)? {
            n += 1;
        }
    }
    if n == 0 {
        return Err(recusa("base de zero linhas".to_string()));
    }
    let ultimo_t = (n as i64 - 1) * passo;
    if ultimo_t + passo < caso.duracao_s {
        return Err(recusa(format!(
            "a última linha (t_s = {ultimo_t}) não cobre a duração do caso ({} s): faltam {} s",
            caso.duracao_s,
            caso.duracao_s - ultimo_t - passo
        )));
    }

    let mut indices = Vec::with_capacity(caso.tomadas.len());
    let mut usadas = HashSet::new();
    for t in &caso.tomadas {
        let idx = colunas
            .iter()
            .position(|c| *c == t.coluna)
            .ok_or_else(|| recusa(format!("coluna \"{}\", usada pela tomada {}, não está no cabeçalho", t.coluna, t.nome)))?;
        usadas.insert(idx);
        indices.push(idx);
    }
    let mut avisos = Vec::new();
    for (i, c) in colunas.iter().enumerate() {
        if !usadas.contains(&i) {
            avisos.push(format!("base {rotulo}: a coluna \"{c}\" não é usada por nenhuma tomada do caso"));
        }
    }

    let sha256 = hex(&hash.finalize());
    Ok(Base { arquivo: caminho.clone(), passo_s: passo, linhas: n, sha256, colunas, indices, avisos })
}
