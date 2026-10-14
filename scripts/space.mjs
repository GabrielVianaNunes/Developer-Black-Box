// Espaço em disco do projeto e dos dados do app: relatório, limpeza e um guarda que impede a pasta `target` de crescer sem fim.
//
// Uso: node scripts/space.mjs report   tamanhos do projeto e dos dados do app (só leitura)
//      node scripts/space.mjs clean    apaga o que é só artefato: target, dist e backups datados do graphify
//      node scripts/space.mjs guard    apaga `target` SÓ se passar do limite (padrão 8 GB; BB_TARGET_LIMIT_GB muda)
//
// `target` (artefatos do cargo) chegou a ~55 GB sem nenhum motivo útil: o cargo nunca limpa os artefatos antigos de cada
// combinação de perfil e feature. Tudo o que este script apaga é recriado por `cargo build`/`npm run build`; nada aqui toca
// código, Git, `node_modules` nem os dados do app (esses só são LIDOS, para o relatório).
import { existsSync, lstatSync, readdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const DEFAULT_TARGET_LIMIT_GB = 8;
const GB = 1024 ** 3;
const MB = 1024 ** 2;

export function formatBytes(n) {
  if (n >= GB) return `${(n / GB).toFixed(2)} GB`;
  return `${(n / MB).toFixed(1)} MB`;
}

/** Limite em GB vindo do ambiente; valor inválido ou não positivo volta ao padrão (nunca desliga o guarda). */
export function limitFromEnv(value) {
  const n = Number(value);
  return Number.isFinite(n) && n > 0 ? n : DEFAULT_TARGET_LIMIT_GB;
}

export function overLimit(bytes, limitGb) {
  return bytes > limitGb * GB;
}

/** Pastas de backup datado do graphify (`2026-10-03`): só estas são removidas de `graphify-out`. */
export function datedBackups(names) {
  return names.filter((n) => /^\d{4}-\d{2}-\d{2}$/.test(n));
}

/** Tamanho de uma pasta, sem seguir links simbólicos. Pasta que não existe vale 0. */
export function dirSize(path) {
  let total = 0;
  let st;
  try {
    st = lstatSync(path);
  } catch {
    return 0;
  }
  if (st.isSymbolicLink()) return 0;
  if (!st.isDirectory()) return st.size;
  let entries;
  try {
    entries = readdirSync(path);
  } catch {
    return 0;
  }
  for (const e of entries) total += dirSize(join(path, e));
  return total;
}

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));

function dataDir() {
  const base = process.env.LOCALAPPDATA ?? join(homedir(), "AppData", "Local");
  return join(base, "DeveloperBlackBox");
}

export function report(log = console.log, base = root) {
  const rows = ["target", "node_modules", "dist", "graphify-out", ".git"].map((p) => [p, dirSize(join(base, p))]);
  log("Projeto:");
  for (const [p, n] of rows) log(`  ${formatBytes(n).padStart(10)}  ${p}`);
  const target = rows[0][1];
  const limit = limitFromEnv(process.env.BB_TARGET_LIMIT_GB);
  if (overLimit(target, limit)) log(`  ! target passou de ${limit} GB: rode "npm run clean" (ou "npm run build", que limpa sozinho).`);
  const data = dataDir();
  if (existsSync(data)) {
    log("Dados do app (só leitura):");
    for (const e of readdirSync(data)) log(`  ${formatBytes(dirSize(join(data, e))).padStart(10)}  ${e}`);
  }
}

function remove(path, log, base = root) {
  const n = dirSize(path);
  if (!existsSync(path)) return 0;
  rmSync(path, { recursive: true, force: true });
  log(`  apagado ${path.replace(base, ".")} (${formatBytes(n)})`);
  return n;
}

export function clean(log = console.log, base = root) {
  let freed = 0;
  freed += remove(join(base, "target"), log, base);
  freed += remove(join(base, "dist"), log, base);
  const g = join(base, "graphify-out");
  if (existsSync(g)) for (const d of datedBackups(readdirSync(g))) freed += remove(join(g, d), log, base);
  log(`Liberado: ${formatBytes(freed)}`);
  return freed;
}

export function guard(log = console.log, base = root, limit = limitFromEnv(process.env.BB_TARGET_LIMIT_GB)) {
  const target = join(base, "target");
  const size = dirSize(target);
  if (!overLimit(size, limit)) return false;
  log(`target tem ${formatBytes(size)} (limite ${limit} GB): apagando artefatos de build, o cargo os recria.`);
  remove(target, log, base);
  return true;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const cmd = process.argv[2];
  if (cmd === "report") report();
  else if (cmd === "clean") clean();
  else if (cmd === "guard") guard();
  else {
    console.error("uso: node scripts/space.mjs report|clean|guard");
    process.exit(2);
  }
}
