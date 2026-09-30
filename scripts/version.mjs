// Versão do app: uma única fonte (Cargo.toml do workspace) e verificação de que nada divergiu.
//
// Uso: node scripts/version.mjs                 mostra a versão atual
//      node scripts/version.mjs check           falha se algum arquivo divergir da fonte
//      node scripts/version.mjs check v0.2.0    idem, e exige que a tag (v0.2.0) seja a versão atual
//      node scripts/version.mjs set 0.2.0       grava a nova versão em todos os arquivos
//      node scripts/version.mjs bump patch      sobe major, minor ou patch (0.1.0 -> 0.1.1)
//
// A versão fica em `[workspace.package]` do Cargo.toml raiz. Os crates e o Tauri herdam dela.
// O `package.json`/`package-lock.json` (npm) e o `Cargo.lock` são mantidos em sincronia por `set`.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-(alpha|beta|rc)\.(0|[1-9]\d*))?$/;

export function isVersion(v) {
  return typeof v === "string" && SEMVER.test(v);
}

export function bump(version, part) {
  const m = SEMVER.exec(version);
  if (!m) throw new Error(`invalid version: ${version}`);
  let [major, minor, patch] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (part === "major") [major, minor, patch] = [major + 1, 0, 0];
  else if (part === "minor") [minor, patch] = [minor + 1, 0];
  else if (part === "patch") patch += 1;
  else throw new Error(`unknown part: ${part} (use major, minor or patch)`);
  return `${major}.${minor}.${patch}`;
}

/** Compara duas versões; retorna <0, 0 ou >0. Pré-lançamento vem antes da versão final. */
export function compare(a, b) {
  const pa = SEMVER.exec(a);
  const pb = SEMVER.exec(b);
  if (!pa || !pb) throw new Error("invalid version");
  for (let i = 1; i <= 3; i++) if (pa[i] !== pb[i]) return Number(pa[i]) - Number(pb[i]);
  if (!pa[4] && !pb[4]) return 0;
  if (!pa[4]) return 1;
  if (!pb[4]) return -1;
  const order = { alpha: 0, beta: 1, rc: 2 };
  return order[pa[5]] - order[pb[5]] || Number(pa[6]) - Number(pb[6]);
}

const read = (root, f) => readFileSync(join(root, f), "utf8");

/** Nome e versão de cada crate do workspace, como o Cargo.lock registra. */
export function lockVersions(lockText, names) {
  const out = {};
  for (const block of lockText.split(/\r?\n\r?\n/)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"/m.exec(block)?.[1];
    if (name && names.includes(name) && !/^source = /m.test(block)) out[name] = version;
  }
  return out;
}

export function workspaceCrates(root) {
  const members = [...read(root, "Cargo.toml").matchAll(/"([^"]+)"/g)].map((m) => m[1]).filter((p) => !p.startsWith("@"));
  return members
    .filter((dir) => /^(crates\/|src-tauri$)/.test(dir))
    .map((dir) => ({ dir, text: read(root, `${dir}/Cargo.toml`) }))
    .map(({ dir, text }) => {
      // Só a seção [package]: dependências também têm linhas `version = "..."`.
      const pkg = /^\[package\]\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(text)[1];
      return {
        dir,
        name: /^name = "([^"]+)"/m.exec(pkg)[1],
        inherits: /^version\.workspace = true\s*$/m.test(pkg),
        own: /^version = "([^"]+)"/m.exec(pkg)?.[1],
      };
    });
}

export function readVersions(root) {
  const cargo = /\[workspace\.package\][^[]*?^version = "([^"]+)"/ms.exec(read(root, "Cargo.toml"))?.[1];
  const pkg = JSON.parse(read(root, "package.json"));
  const lock = JSON.parse(read(root, "package-lock.json"));
  const tauri = JSON.parse(read(root, "src-tauri/tauri.conf.json"));
  return { cargo, pkg: pkg.version, lock: lock.version, lockRoot: lock.packages?.[""]?.version, tauri: tauri.version };
}

/** Lista de problemas; vazia quando tudo está consistente. */
export function check(root, tag) {
  const problems = [];
  const v = readVersions(root);
  if (!isVersion(v.cargo)) problems.push(`Cargo.toml [workspace.package] version is missing or not semver: ${v.cargo}`);
  for (const [k, file] of [["pkg", "package.json"], ["lock", "package-lock.json"], ["lockRoot", "package-lock.json (packages[''])"]]) {
    if (v[k] !== v.cargo) problems.push(`${file} is ${v[k]}, expected ${v.cargo}`);
  }
  if (v.tauri !== undefined) problems.push("tauri.conf.json must not set its own version (it inherits from Cargo.toml)");
  const crates = workspaceCrates(root);
  for (const c of crates) if (!c.inherits || c.own) problems.push(`${c.dir}/Cargo.toml must use version.workspace = true`);
  const locked = lockVersions(read(root, "Cargo.lock"), crates.map((c) => c.name));
  for (const c of crates) if (locked[c.name] !== v.cargo) problems.push(`Cargo.lock has ${c.name} ${locked[c.name]}, expected ${v.cargo}`);
  if (tag !== undefined) {
    if (tag !== `v${v.cargo}`) problems.push(`tag ${tag} does not match version v${v.cargo}`);
    const log = read(root, "CHANGELOG.md");
    if (!new RegExp(`^## \\[${v.cargo.replaceAll(".", "\\.")}\\] - \\d{4}-\\d{2}-\\d{2}\\s*$`, "m").test(log)) {
      problems.push(`CHANGELOG.md has no dated "## [${v.cargo}] - YYYY-MM-DD" section`);
    }
  }
  return problems;
}

export function setVersion(root, version, { refreshLock = true } = {}) {
  if (!isVersion(version)) throw new Error(`invalid version: ${version} (expected x.y.z or x.y.z-rc.1)`);
  const cargoPath = join(root, "Cargo.toml");
  const cargo = read(root, "Cargo.toml").replace(/(\[workspace\.package\][^[]*?^version = ")[^"]+(")/ms, `$1${version}$2`);
  writeFileSync(cargoPath, cargo);
  for (const f of ["package.json", "package-lock.json"]) {
    const text = read(root, f);
    const json = JSON.parse(text);
    json.version = version;
    if (json.packages?.[""]) json.packages[""].version = version;
    writeFileSync(join(root, f), JSON.stringify(json, null, 2) + (text.endsWith("\n") ? "\n" : ""));
  }
  if (!refreshLock) return;
  // Atualiza o Cargo.lock. Tenta sem rede (mais rápido); se faltar algum pacote em cache, tenta com rede.
  const run = (extra) =>
    execFileSync("cargo", ["metadata", "--format-version", "1", ...extra], { cwd: root, stdio: "ignore", maxBuffer: 256 * 1024 * 1024 });
  try {
    run(["--offline"]);
  } catch {
    run([]);
  }
}

function main(argv) {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const [cmd, arg] = argv;
  if (!cmd) return console.log(readVersions(root).cargo);
  if (cmd === "check") {
    const problems = check(root, arg);
    if (problems.length) {
      console.error(problems.map((p) => `FAIL: ${p}`).join("\n"));
      process.exit(1);
    }
    return console.log(`OK: version ${readVersions(root).cargo} is consistent${arg ? ` with tag ${arg}` : ""}.`);
  }
  if (cmd === "set" || cmd === "bump") {
    const current = readVersions(root).cargo;
    const next = cmd === "set" ? arg : bump(current, arg);
    if (cmd === "set" && isVersion(next) && compare(next, current) <= 0) {
      console.error(`FAIL: ${next} is not greater than the current version ${current}.`);
      process.exit(1);
    }
    setVersion(root, next);
    return console.log(`Version ${current} -> ${next}. Update CHANGELOG.md, then commit.`);
  }
  console.error("usage: version.mjs [check [tag] | set <x.y.z> | bump <major|minor|patch>]");
  process.exit(2);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(`FAIL: ${e.message}`);
    process.exit(1);
  }
}
