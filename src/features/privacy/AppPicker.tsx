import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { listAppCandidates, pickExecutable } from "../../services/backend";
import type { AppCandidate } from "../../types/dashboard";
import { filterCandidates } from "./filterCandidates";

const CACHE_MS = 30_000;

// A lista é montada uma vez e compartilhada entre as duas listas de privacidade por alguns segundos.
// Só vive na memória desta tela: nunca é gravada nem enviada.
let cache: { at: number; promise: Promise<AppCandidate[]> } | null = null;

function loadCandidates(): Promise<AppCandidate[]> {
  if (cache && Date.now() - cache.at < CACHE_MS) return cache.promise;
  const promise = listAppCandidates();
  cache = { at: Date.now(), promise };
  promise.catch(() => {
    if (cache?.promise === promise) cache = null; // falhou: a próxima tentativa busca de novo
  });
  return promise;
}

/** Candidatos (ou null enquanto carrega) e o erro de listagem, se houver. */
export function useAppCandidates(enabled: boolean) {
  const [list, setList] = useState<AppCandidate[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!enabled || list) return;
    let alive = true;
    loadCandidates()
      .then((l) => alive && setList(l))
      .catch((e) => alive && setError(typeof e === "string" ? e : "apps.list_failed"));
    return () => {
      alive = false;
    };
  }, [enabled, list]);
  return { list, error };
}

/**
 * Escolha de programa SEM texto livre: busca entre os programas deste PC e só adiciona quem for selecionado.
 * Para um programa que não aparece, "Procurar arquivo…" abre o seletor do Windows (só o nome do .exe é guardado).
 */
export function AppPicker({ taken, onAdd }: { taken: string[]; onAdd: (exe: string) => void }) {
  const { t, errorText } = useI18n();
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [wanted, setWanted] = useState(false); // só lista os programas quando o campo é usado
  const [msg, setMsg] = useState<string | null>(null);
  const { list, error } = useAppCandidates(wanted);
  const listId = useId();
  const inputRef = useRef<HTMLInputElement>(null);

  const options = useMemo(() => (list ? filterCandidates(list, query, taken) : []), [list, query, taken]);

  function choose(c: AppCandidate) {
    onAdd(c.exe);
    setQuery("");
    setOpen(false);
    setActive(0);
    setMsg(null);
  }

  async function browse() {
    setMsg(null);
    try {
      const exe = await pickExecutable(t("privacy.pickTitle"), t("privacy.pickFilter"));
      if (exe && !taken.includes(exe)) onAdd(exe);
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setOpen(true);
      setActive((i) => Math.min(i + 1, Math.max(options.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (open && options[active]) choose(options[active]);
    } else if (e.key === "Escape") {
      setOpen(false);
    }
  }

  const showList = open && wanted;
  return (
    <div className="picker">
      <div className="row">
        <div className="picker-field">
          <input
            ref={inputRef}
            className="grow"
            role="combobox"
            aria-expanded={showList}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={showList && options[active] ? `${listId}-${active}` : undefined}
            aria-label={t("privacy.searchLabel")}
            placeholder={t("privacy.searchPlaceholder")}
            value={query}
            autoComplete="off"
            spellCheck={false}
            onFocus={() => {
              setWanted(true);
              setOpen(true);
            }}
            onBlur={() => setOpen(false)}
            onChange={(e) => {
              setQuery(e.target.value);
              setOpen(true);
              setActive(0);
            }}
            onKeyDown={onKeyDown}
          />
          {showList && (
            <ul id={listId} role="listbox" className="picker-list">
              {!list && !error && <li className="picker-note muted small">{t("privacy.loadingApps")}</li>}
              {error && <li className="picker-note notice">{errorText(error)}</li>}
              {list && options.length === 0 && <li className="picker-note muted small">{t("privacy.noMatch")}</li>}
              {options.map((c, i) => (
                <li
                  key={c.exe}
                  id={`${listId}-${i}`}
                  role="option"
                  aria-selected={i === active}
                  className={i === active ? "picker-option active" : "picker-option"}
                  // mousedown (e não click): o campo perderia o foco e fecharia a lista antes do clique.
                  onMouseDown={(e) => {
                    e.preventDefault();
                    choose(c);
                  }}
                  onMouseEnter={() => setActive(i)}
                >
                  <span className="picker-name">{c.name}</span>
                  <span className="muted small">{c.exe}</span>
                  {c.running && <span className="badge">{t("privacy.running")}</span>}
                </li>
              ))}
            </ul>
          )}
        </div>
        <button onClick={browse}>{t("privacy.browse")}</button>
      </div>
      {msg && <p className="notice" role="status">{msg}</p>}
    </div>
  );
}
