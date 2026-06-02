// Edge Function — Provisionamento e Renovação de credenciais por-dispositivo.
// Feature 001-otimizacao-tomadas-milp · Constituição Princípio VIII.
// Contrato: specs/001-otimizacao-tomadas-milp/contracts/provisioning-api.md
//
// Endpoints (POST):
//   /functions/v1/provisionar            -> provisionar(mac, claim_code)
//   /functions/v1/provisionar/renovar    -> renovar(tomada_id, device_secret)
//
// Segurança (não-negociável):
//   * Único ponto que assina JWT (segredo de assinatura NUNCA vai ao firmware).
//   * Usa service_role só aqui; tabelas têm RLS default-deny.
//   * Guarda apenas sha256(segredo) / sha256(claim_code); compara em tempo constante.
//   * device_secret gerado por CSPRNG (32 bytes).
//
// Secrets necessários (supabase secrets set ...):
//   DEVICE_JWT_JWK  -> a chave de assinatura ES256 (JWK PRIVADA, com `d`/`kid`) gerada por
//                      `supabase gen signing-key --algorithm ES256`. A MESMA chave (parte
//                      pública) deve ser importada nas JWT Signing Keys do projeto para que
//                      ela entre no JWKS e o PostgREST/RLS verifique os tokens emitidos aqui.
//                      Sem isso a RLS rejeita o token. O `kid` do header sai do próprio JWK.
//   (SUPABASE_URL e SUPABASE_SERVICE_ROLE_KEY são injetados automaticamente.)

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.45.4";
import { create, getNumericDate } from "https://deno.land/x/djwt@v3.0.2/mod.ts";

// ----------------------------------------------------------------------------
// Config
// ----------------------------------------------------------------------------
const SUPABASE_URL = Deno.env.get("SUPABASE_URL")!;
const SERVICE_ROLE_KEY = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!;
// JWK privada ES256 (P-256) usada para assinar os JWTs por-dispositivo. A parte pública
// desta MESMA chave fica no JWKS do projeto (importada nas JWT Signing Keys).
const DEVICE_JWT_JWK = Deno.env.get("DEVICE_JWT_JWK") ?? "";
const JWT_TTL_SECONDS = Number(Deno.env.get("JWT_TTL_SECONDS") ?? "3600"); // 1h

// MODO AUTO (demo single-tenant, "conectar e usar" sem login): quando ON, um device que
// chama /provisionar SEM claim_code e registrado numa residencia padrao. NAO ha isolamento
// multi-inquilino nesse modo (qualquer MAC entra). Default ON p/ a demo; desligue
// (AUTO_PROVISION=false) para exigir claim-code (modo seguro multi-inquilino).
const AUTO_PROVISION = (Deno.env.get("AUTO_PROVISION") ?? "true") === "true";
// Residencia padrao do modo auto. Se vazio, usa a 1a existente ou cria "Casa Demo".
const DEFAULT_RESIDENCIA_ID = Deno.env.get("DEFAULT_RESIDENCIA_ID") ?? "";

const MAC_RE = /^([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}$/;

// ----------------------------------------------------------------------------
// Crypto helpers
// ----------------------------------------------------------------------------
const enc = new TextEncoder();

async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", enc.encode(input));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function randomSecretHex(bytes = 32): string {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return [...buf].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// Comparação em tempo constante de duas strings hex de mesmo tamanho esperado.
function timingSafeEqualHex(a: string, b: string): boolean {
  // Normaliza para o maior tamanho para não vazar tamanho via early-return.
  const len = Math.max(a.length, b.length);
  let diff = a.length ^ b.length;
  for (let i = 0; i < len; i++) {
    diff |= (a.charCodeAt(i) || 0) ^ (b.charCodeAt(i) || 0);
  }
  return diff === 0;
}

// Chave de assinatura ES256 importada uma vez por isolate (a partir do JWK em DEVICE_JWT_JWK).
interface SigningKey {
  key: CryptoKey;
  kid: string;
}
let _signingKey: SigningKey | null = null;

async function getSigningKey(): Promise<SigningKey> {
  if (_signingKey) return _signingKey;
  const raw = JSON.parse(DEVICE_JWT_JWK);
  if (!raw.kid) throw new Error("DEVICE_JWT_JWK sem 'kid'");
  if (!raw.d) throw new Error("DEVICE_JWT_JWK nao e chave privada (falta 'd'); cole a JWK do 'gen signing-key', nao a publica do JWKS");
  // Reconstrói uma JWK mínima e limpa: evita conflito de importKey com campos como
  // `key_ops:["verify"]`/`use`/`ext`/`alg` que vêm na versão pública ou do dashboard.
  const jwk = { kty: raw.kty, crv: raw.crv, d: raw.d, x: raw.x, y: raw.y };
  const key = await crypto.subtle.importKey(
    "jwk",
    jwk,
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign"],
  );
  _signingKey = { key, kid: jwk.kid };
  return _signingKey;
}

async function mintDeviceJwt(
  signing: SigningKey,
  tomadaId: string,
  residenciaId: string,
): Promise<{ jwt: string; jwt_exp: number }> {
  const exp = getNumericDate(JWT_TTL_SECONDS);
  const jwt = await create(
    // O `kid` casa a chave pública correspondente no JWKS do projeto (verificação ES256).
    { alg: "ES256", kid: signing.kid, typ: "JWT" },
    {
      role: "device", // PostgREST faz SET ROLE device (menor privilégio)
      aud: "authenticated",
      sub: tomadaId,
      tomada_id: tomadaId, // claims usados pelas policies RLS
      residencia_id: residenciaId,
      iat: getNumericDate(0),
      exp,
    },
    signing.key,
  );
  return { jwt, jwt_exp: exp };
}

// ----------------------------------------------------------------------------
// HTTP helpers
// ----------------------------------------------------------------------------
function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

// Rate-limit best-effort por isolate (memória do worker). Para produção robusta,
// usar um store durável (ex.: tabela/Redis); aqui mitiga rajadas no mesmo worker.
const HITS = new Map<string, { n: number; resetAt: number }>();
const RL_WINDOW_MS = 60_000;
const RL_MAX = 20;
function rateLimited(keyId: string): boolean {
  const now = Date.now();
  const cur = HITS.get(keyId);
  if (!cur || now > cur.resetAt) {
    HITS.set(keyId, { n: 1, resetAt: now + RL_WINDOW_MS });
    return false;
  }
  cur.n++;
  return cur.n > RL_MAX;
}

// ----------------------------------------------------------------------------
// Handlers
// ----------------------------------------------------------------------------
const supabase = createClient(SUPABASE_URL, SERVICE_ROLE_KEY, {
  auth: { persistSession: false, autoRefreshToken: false },
});

// Modo AUTO: resolve a residência padrão — env DEFAULT_RESIDENCIA_ID, senão a 1a existente,
// senão cria "Casa Demo" (owner_id null, permitido pela migração do modo demo).
async function resolveDefaultResidencia(): Promise<string> {
  if (DEFAULT_RESIDENCIA_ID) return DEFAULT_RESIDENCIA_ID;

  const { data: existente } = await supabase
    .from("residencias")
    .select("id")
    .order("criado_em", { ascending: true })
    .limit(1)
    .maybeSingle();
  if (existente) return existente.id;

  const { data: nova, error } = await supabase
    .from("residencias")
    .insert({ nome: "Casa Demo", p_max: 3000 })
    .select("id")
    .single();
  if (error || !nova) throw new Error(`falha criando residência padrão: ${error?.message}`);
  return nova.id;
}

// Chave HMAC compartilhada da residência para autenticar o ESP-NOW (protocolo v3).
// Lazy: gera na 1ª vez e reusa para todos os devices da MESMA residência (todos precisam
// da mesma chave para se reconhecerem). Entregue ao device SÓ aqui — a tabela
// `residencia_secrets` é default-deny (nunca lida pela Data API). Princípio VIII.
async function getOrCreateEspnowKey(residenciaId: string): Promise<string> {
  const { data: existente } = await supabase
    .from("residencia_secrets")
    .select("espnow_key_hex")
    .eq("residencia_id", residenciaId)
    .maybeSingle();
  if (existente?.espnow_key_hex) return existente.espnow_key_hex;

  const key = randomSecretHex(32); // 32 bytes -> 64 hex chars
  const { error } = await supabase
    .from("residencia_secrets")
    .upsert({ residencia_id: residenciaId, espnow_key_hex: key }, { onConflict: "residencia_id" });
  if (error) throw new Error(`falha gerando chave ESP-NOW: ${error.message}`);
  return key;
}

async function handleProvisionar(req: Request): Promise<Response> {
  let payload: { mac?: string; claim_code?: string };
  try {
    payload = await req.json();
  } catch {
    return json({ error: "json inválido" }, 400);
  }
  const mac = (payload.mac ?? "").trim().toUpperCase();
  const claimCode = payload.claim_code ?? "";
  if (!MAC_RE.test(mac)) {
    return json({ error: "mac inválido" }, 400);
  }
  if (rateLimited(`prov:${mac}`)) return json({ error: "rate limited" }, 429);

  // 1. Resolver a residência: por CLAIM (modo seguro) ou AUTO (modo demo, sem login).
  let residenciaId: string;
  let claimId: string | null = null;
  if (claimCode.length >= 6) {
    // --- MODO CLAIM (seguro / multi-inquilino) ---
    const codeHash = await sha256Hex(claimCode);
    const { data: claim, error: claimErr } = await supabase
      .from("provisioning_claims")
      .select("id, residencia_id, expira_em, usado_em")
      .eq("code_hash", codeHash)
      .maybeSingle();
    if (claimErr) {
      console.error("[provisionar] claimErr:", claimErr); // observabilidade (FR-017), sem vazar ao cliente
      return json({ error: "erro ao validar claim" }, 500);
    }
    if (!claim || claim.usado_em !== null || new Date(claim.expira_em) < new Date()) {
      return json({ error: "claim inválido, expirado ou usado" }, 401);
    }
    residenciaId = claim.residencia_id;
    claimId = claim.id;
  } else if (AUTO_PROVISION) {
    // --- MODO AUTO (demo single-tenant): joga numa residência padrão, sem claim ---
    try {
      residenciaId = await resolveDefaultResidencia();
    } catch (e) {
      console.error("[provisionar] auto residência:", e);
      return json({ error: "falha resolvendo residência padrão" }, 500);
    }
  } else {
    return json({ error: "claim_code obrigatório" }, 401);
  }

  // 2. Upsert da tomada pelo MAC (sem sobrescrever `nome` de uma já existente).
  const { data: existente } = await supabase
    .from("tomadas")
    .select("id")
    .eq("mac", mac)
    .maybeSingle();

  let tomadaId: string;
  if (existente) {
    tomadaId = existente.id;
    await supabase
      .from("tomadas")
      .update({ residencia_id: residenciaId })
      .eq("id", tomadaId);
  } else {
    const nome = `Tomada ${mac.slice(-5)}`;
    const { data: nova, error: insErr } = await supabase
      .from("tomadas")
      .insert({ mac, residencia_id: residenciaId, nome })
      .select("id")
      .single();
    if (insErr || !nova) return json({ error: "falha ao registrar tomada" }, 500);
    tomadaId = nova.id;
  }

  // 3. Gerar segredo durável e gravar só o hash (revoga/rotaciona o anterior).
  const deviceSecret = randomSecretHex(32);
  const secretHash = await sha256Hex(deviceSecret);
  const { error: credErr } = await supabase.from("device_credentials").upsert(
    {
      tomada_id: tomadaId,
      secret_hash: secretHash,
      revoked_at: null,
      ultima_renovacao: null,
    },
    { onConflict: "tomada_id" },
  );
  if (credErr) return json({ error: "falha ao gravar credencial" }, 500);

  // 4. Consumir o claim (uso único, com guarda contra corrida). Só no modo claim.
  if (claimId) {
    const { data: usado, error: useErr } = await supabase
      .from("provisioning_claims")
      .update({ usado_em: new Date().toISOString() })
      .eq("id", claimId)
      .is("usado_em", null)
      .select("id")
      .maybeSingle();
    if (useErr) return json({ error: "falha ao consumir claim" }, 500);
    if (!usado) return json({ error: "claim já consumido" }, 409);
  }

  // 5. Emitir o 1º JWT.
  const signing = await getSigningKey();
  const { jwt, jwt_exp } = await mintDeviceJwt(signing, tomadaId, residenciaId);

  // 6. Chave HMAC compartilhada da residência (ESP-NOW v3). Mesma para todos os devices
  //    da casa; entregue só aqui e gravada em LittleFS no firmware.
  let residenciaKey: string;
  try {
    residenciaKey = await getOrCreateEspnowKey(residenciaId);
  } catch (e) {
    console.error("[provisionar] espnow key:", e);
    return json({ error: "falha gerando chave de coordenação" }, 500);
  }

  return json({
    tomada_id: tomadaId,
    residencia_id: residenciaId,
    device_secret: deviceSecret, // mostrado só aqui
    residencia_key: residenciaKey, // chave ESP-NOW da casa (32 bytes hex)
    jwt,
    jwt_exp,
  });
}

async function handleRenovar(req: Request): Promise<Response> {
  let payload: { tomada_id?: string; device_secret?: string };
  try {
    payload = await req.json();
  } catch {
    return json({ error: "json inválido" }, 400);
  }
  const tomadaId = payload.tomada_id ?? "";
  const deviceSecret = payload.device_secret ?? "";
  if (!tomadaId || deviceSecret.length < 16) {
    return json({ error: "tomada_id ou device_secret inválido" }, 400);
  }
  if (rateLimited(`renov:${tomadaId}`)) return json({ error: "rate limited" }, 429);

  const { data: cred, error: credErr } = await supabase
    .from("device_credentials")
    .select("secret_hash, revoked_at")
    .eq("tomada_id", tomadaId)
    .maybeSingle();
  if (credErr) {
    console.error("[provisionar] credErr:", credErr); // observabilidade (FR-017), sem vazar ao cliente
    return json({ error: "erro ao ler credencial" }, 500);
  }
  if (!cred) return json({ error: "tomada desconhecida" }, 404);
  if (cred.revoked_at !== null) return json({ error: "credencial revogada" }, 401);

  const presentedHash = await sha256Hex(deviceSecret);
  if (!timingSafeEqualHex(presentedHash, cred.secret_hash)) {
    return json({ error: "segredo inválido" }, 401);
  }

  // Resolver residencia_id para os claims.
  const { data: tomada, error: tErr } = await supabase
    .from("tomadas")
    .select("residencia_id")
    .eq("id", tomadaId)
    .maybeSingle();
  if (tErr || !tomada) return json({ error: "tomada desconhecida" }, 404);

  await supabase
    .from("device_credentials")
    .update({ ultima_renovacao: new Date().toISOString() })
    .eq("tomada_id", tomadaId);

  const signing = await getSigningKey();
  const { jwt, jwt_exp } = await mintDeviceJwt(signing, tomadaId, tomada.residencia_id);
  return json({ jwt, jwt_exp });
}

// ----------------------------------------------------------------------------
// Router
// ----------------------------------------------------------------------------
Deno.serve(async (req: Request) => {
  if (req.method !== "POST") return json({ error: "method not allowed" }, 405);
  if (!DEVICE_JWT_JWK) return json({ error: "DEVICE_JWT_JWK não configurado" }, 500);

  const path = new URL(req.url).pathname.replace(/\/+$/, "");
  const action = path.split("/").pop(); // "provisionar" | "renovar"

  try {
    if (action === "renovar") return await handleRenovar(req);
    return await handleProvisionar(req); // default: provisionar
  } catch (e) {
    console.error("[provisionar] erro inesperado:", e); // detalhe só no log (FR-017), nunca ao cliente
    return json({ error: "erro interno" }, 500);
  }
});
