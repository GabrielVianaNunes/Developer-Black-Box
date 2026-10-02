// Pasta de instalação: o padrão é a recomendada, a escolha de pasta continua disponível e as pastas que sabemos que
// não funcionam são recusadas (sem nunca travar uma atualização). A lógica é compilada de verdade com o makensis
// (o do Tauri) num instalador-miniatura; sem o makensis na máquina, só as checagens estáticas rodam.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const hooksPath = join(root, "src-tauri", "windows", "hooks.nsh");
const conf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const hooks = readFileSync(hooksPath, "utf8");

test("the installer is per-user (default folder under Local\\Programs) and runs the folder check before copying files", () => {
  assert.equal(conf.bundle.windows.nsis.installMode, "currentUser");
  assert.equal(conf.bundle.windows.nsis.installerHooks, "windows/hooks.nsh");
  assert.ok(/!macro NSIS_HOOK_PREINSTALL\s+!insertmacro BB_CHECK_INSTDIR/.test(hooks));
  assert.ok(/\$UpdateMode <> 1/.test(hooks), "updates are never refused");
  assert.ok(/MessageBox[^\n]*\/SD IDOK/.test(hooks), "silent installs do not hang on the message");
  assert.ok(!/RMDir \/r/i.test(hooks), "the refusal never deletes recursively");
});

test("after installing or updating, the installer asks Windows to refresh icons so the taskbar button does not keep an old cached icon", () => {
  assert.ok(/!macro NSIS_HOOK_POSTINSTALL\s+System::Call 'shell32::SHChangeNotify\(i 0x08000000, i 0x1000, p 0, p 0\)'/.test(hooks), "SHCNE_ASSOCCHANGED after install");
  const build = readFileSync(join(root, "src-tauri", "build.rs"), "utf8");
  assert.ok(/encode_ico_cube/.test(build) && !/Light::Gray/.test(build), "the executable icon is the cube without any light");
  const lib = readFileSync(join(root, "src-tauri", "src", "lib.rs"), "utf8");
  assert.ok(/set_overlay_icon\(Some\(Image::new_owned\(icon::render_dot\(light, size\)/.test(lib), "the state color goes in the taskbar badge");
});

test("restarting File Explorer is only offered, never forced: Yes/No question, default No when silent, and only after an old-icon version", () => {
  assert.ok(/MessageBox MB_YESNO\|MB_ICONQUESTION "\$R9" \/SD IDNO IDNO bb_skip_explorer_restart/.test(hooks), "a question, silent default No");
  const kill = hooks.indexOf("taskkill /f /im explorer.exe");
  const ask = hooks.indexOf("MessageBox MB_YESNO|MB_ICONQUESTION");
  const guard = hooks.indexOf("${If} $BBIconRefresh = 1");
  assert.ok(guard > 0 && ask > guard && kill > ask, "the kill comes after the guard and after the question");
  assert.equal(hooks.split("taskkill").length - 1, 1, "Explorer is stopped in exactly one place");
  assert.ok(/ReadRegStr \$R0 SHCTX "\$\{UNINSTKEY\}" "DisplayVersion"/.test(hooks), "the previous version is read before installing");
});

const nsis = [process.env.BB_MAKENSIS, join(process.env.LOCALAPPDATA ?? "", "tauri", "NSIS", "makensis.exe")].find((p) => p && existsSync(p));

test("new installs into Program Files or straight into AppData are refused; the suggested folder and others are accepted; updates always pass", { skip: !nsis && "makensis not available" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "bb-nsis-"));
  try {
    const src = [
      "Unicode true",
      "RequestExecutionLevel user",
      "SilentInstall silent",
      'OutFile "harness.exe"',
      "Var UpdateMode",
      "!include LogicLib.nsh",
      `!include "${hooksPath}"`,
      "Section",
      '  ReadEnvStr $UpdateMode BB_UPDATE',
      '  StrCmp $UpdateMode "" 0 +2',
      "    StrCpy $UpdateMode 0",
      "  !insertmacro BB_CHECK_INSTDIR",
      "SectionEnd",
      "",
    ].join("\r\n");
    writeFileSync(join(dir, "harness.nsi"), src);
    const built = spawnSync(nsis, ["harness.nsi"], { cwd: dir, encoding: "utf8" });
    assert.equal(built.status, 0, built.stdout + built.stderr);

    const local = process.env.LOCALAPPDATA;
    const roaming = process.env.APPDATA;
    const exit = (folder, update) => {
      const env = { ...process.env };
      delete env.BB_UPDATE;
      if (update) env.BB_UPDATE = "1";
      const r = spawnSync(`"${join(dir, "harness.exe")}" /S /D=${folder}`, { env, shell: true, windowsVerbatimArguments: true });
      return r.status;
    };
    const REFUSED = 2;
    for (const f of ["C:\\Program Files\\Black", "C:\\Program Files (x86)\\Black", "C:\\Program Files", join(local, "Developer Black Box"), join(roaming, "Developer Black Box"), local, join(local, "Programs")]) {
      assert.equal(exit(f, false), REFUSED, `refused: ${f}`);
    }
    for (const f of [join(local, "Programs", "Developer Black Box"), "C:\\Users\\someone\\Apps\\Developer Black Box", "D:\\Black Box", "C:\\Program Files Extra\\Black"]) {
      assert.equal(exit(f, false), 0, `accepted: ${f}`);
    }
    for (const f of ["C:\\Program Files\\Black", join(local, "Developer Black Box")]) {
      assert.equal(exit(f, true), 0, `an update is never refused: ${f}`);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the previous version decides whether to offer the Explorer restart: 0.1.x, 0.2.x and 0.3.0 to 0.3.3 yes; 0.3.4 and later no", { skip: !nsis && "makensis not available" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "bb-nsis-icon-"));
  try {
    const src = [
      "Unicode true",
      "RequestExecutionLevel user",
      "SilentInstall silent",
      'OutFile "harness.exe"',
      "Var UpdateMode",
      "!include LogicLib.nsh",
      `!include "${hooksPath}"`,
      "Section",
      "  ReadEnvStr $R0 BB_PREV",
      "  !insertmacro BB_CLASSIFY_PREVIOUS_ICON",
      "  ${If} $BBIconRefresh = 1",
      "    SetErrorLevel 7",
      "  ${Else}",
      "    SetErrorLevel 0",
      "  ${EndIf}",
      "SectionEnd",
      "",
    ].join("\r\n");
    writeFileSync(join(dir, "harness.nsi"), src);
    const built = spawnSync(nsis, ["harness.nsi"], { cwd: dir, encoding: "utf8" });
    assert.equal(built.status, 0, built.stdout + built.stderr);
    const exit = (prev) => {
      const env = { ...process.env };
      delete env.BB_PREV;
      if (prev) env.BB_PREV = prev;
      return spawnSync(`"${join(dir, "harness.exe")}" /S`, { env, shell: true, windowsVerbatimArguments: true }).status;
    };
    for (const v of ["0.1.0", "0.1.1", "0.1.1-rc.1", "0.1.2", "0.2.0", "0.3.0", "0.3.1", "0.3.2", "0.3.3"]) assert.equal(exit(v), 7, `offer for ${v}`);
    for (const v of ["0.3.4", "0.3.5", "0.3.10", "0.30.0", "0.4.0", "1.0.0", ""]) assert.equal(exit(v), 0, `no offer for ${v || "(none)"}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
