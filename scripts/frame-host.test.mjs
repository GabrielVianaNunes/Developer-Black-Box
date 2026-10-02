// Apps UWP e o ApplicationFrameHost (#66): a leitura do primeiro plano resolve o app de verdade e nunca lê títulos.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(root + p, "utf8");

test("the foreground reader resolves the app behind the frame host and treats a failure as unknown (fail closed)", () => {
  const win = read("crates/bb-collector/src/windows_impl.rs");
  const fn = win.slice(win.indexOf("fn foreground_exe()"), win.indexOf("fn exe_name_of"));
  assert.ok(/name\.as_str\(\) != FRAME_HOST_EXE/.test(fn), "only the frame host is treated specially");
  assert.ok(/let app_pid = resolve_hosted\(pid, &hosted_children\(hwnd\)\)\?;/.test(fn), "an unresolved app returns None through `?`, never the host's name");
  assert.ok(/exe_name_of\(app_pid\)/.test(fn));
});

test("only window classes and process ids are read from the child windows, never titles", () => {
  for (const f of ["crates/bb-collector/src/windows_impl.rs", "crates/bb-collector/src/hosted.rs"]) {
    const code = read(f).replace(/\/\/.*$/gm, "");
    assert.ok(!/GetWindowText|InternalGetWindowText|SendMessage|WM_GETTEXT/i.test(code), `${f} must never read a window title`);
  }
  const win = read("crates/bb-collector/src/windows_impl.rs");
  assert.ok(/GetClassNameW\(child, &mut class\)/.test(win) && /CORE_WINDOW_CLASS/.test(win));
});

test("the decision is a pure function with the constants the Windows part relies on", () => {
  const hosted = read("crates/bb-collector/src/hosted.rs");
  assert.ok(/pub const FRAME_HOST_EXE: &str = "applicationframehost\.exe";/.test(hosted));
  assert.ok(/pub const CORE_WINDOW_CLASS: &str = "Windows\.UI\.Core\.CoreWindow";/.test(hosted));
  assert.ok(/pub mod hosted;/.test(read("crates/bb-collector/src/lib.rs")));
});
