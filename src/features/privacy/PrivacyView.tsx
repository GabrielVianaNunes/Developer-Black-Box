import { useEffect, useState } from "react";
import { CONFIG_CHANGE_LABEL, CONFIG_KEY_LABEL, fmtTime } from "../../app/format";
import { usePolling } from "../../app/hooks";
import { getConfigHistory, getSettings, setSettings } from "../../services/backend";
import type { Settings } from "../../types/dashboard";
import type { Status } from "../../types/status";
import { AuthorizationsCard } from "./AuthorizationsCard";
import { StartupCard } from "./StartupCard";

export function PrivacyView({ status }: { status: Status }) {
  const [saved, setSaved] = useState<Settings | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const history = usePolling(getConfigHistory, 5000);

  useEffect(() => {
    getSettings().then((s) => {
      setSaved(s);
      setDraft(s);
    });
  }, []);

  if (!draft || !saved) return <p className="muted">Carregando…</p>;
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);

  async function save() {
    if (!draft) return;
    setMsg(null);
    try {
      const s = await setSettings(draft);
      setSaved(s);
      setDraft(s);
      setMsg("Configurações aplicadas. A regra nova já vale; a gravação só volta depois de uma nova janela de estabilidade.");
      void history.reload();
    } catch (e) {
      setMsg(typeof e === "string" ? e : "Não foi possível salvar.");
    }
  }

  return (
    <section aria-label="Privacidade">
      <div className="card">
        <h2>Privacy Guard</h2>
        <p>
          Estado atual: <strong>{status.text}</strong>
        </p>
        <p className="muted">
          O Guard avalia o contexto o tempo todo, inclusive durante a pausa manual. Ausência de sinal nunca é tratada como
          seguro, e uma falha do detector suspende a gravação.
        </p>
      </div>

      <AppList
        title="Aplicativos protegidos"
        help="Se um destes estiver em primeiro plano, a gravação é suspensa e nada dele é gravado. Remover um item reduz a proteção."
        items={draft.protectedApps}
        onChange={(v) => setDraft({ ...draft, protectedApps: v })}
      />
      <AppList
        title="Regras de exclusão"
        help="Eventos destes aplicativos nunca são gravados (a gravação dos demais continua)."
        items={draft.excludedApps}
        onChange={(v) => setDraft({ ...draft, excludedApps: v })}
      />

      <div className="card">
        <h2>Configurações de gravação</h2>
        <label className="inline">
          Janela de estabilidade (segundos)
          <input
            type="number"
            min={1}
            max={60}
            value={draft.stabilityWindowMs / 1000}
            onChange={(e) => setDraft({ ...draft, stabilityWindowMs: Math.round(Number(e.target.value) * 1000) })}
          />
        </label>
        <p className="muted small">Tempo contínuo de contexto seguro exigido antes de gravar ou retomar.</p>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.autoStart}
            onChange={(e) => setDraft({ ...draft, autoStart: e.target.checked })}
          />
          Começar a gravar ao abrir o aplicativo
        </label>
        <p className="muted small">
          Desligado por padrão: sem isso, o app abre pausado e só grava depois que você clicar em "Retomar gravação".
        </p>
        <div className="row">
          <button className="primary" disabled={!dirty} onClick={save}>Aplicar</button>
          <button disabled={!dirty} onClick={() => setDraft(saved)}>Descartar</button>
        </div>
        {msg && <p className="notice" role="status">{msg}</p>}
      </div>

      <StartupCard />

      <AuthorizationsCard protectedApps={saved.protectedApps} />

      <div className="card">
        <h2>Histórico de alterações</h2>
        <p className="muted small">Só a configuração e o tipo da mudança; os valores não são registrados.</p>
        <ul className="plain">
          {(history.data ?? []).map((h, i) => (
            <li key={i}>
              <span className="muted small">{fmtTime(h.atUtcMs)}</span> {CONFIG_KEY_LABEL[h.key] ?? h.key}:{" "}
              {CONFIG_CHANGE_LABEL[h.change] ?? h.change}
            </li>
          ))}
        </ul>
        {history.data && history.data.length === 0 && <p className="muted">Nenhuma alteração.</p>}
      </div>
    </section>
  );
}

function AppList({
  title,
  help,
  items,
  onChange,
}: {
  title: string;
  help: string;
  items: string[];
  onChange: (v: string[]) => void;
}) {
  const [name, setName] = useState("");
  const add = () => {
    const n = name.trim().toLowerCase();
    if (n && !items.includes(n)) onChange([...items, n].sort());
    setName("");
  };
  return (
    <div className="card">
      <h2>{title}</h2>
      <p className="muted small">{help}</p>
      <div className="chips">
        {items.map((a) => (
          <span key={a} className="chip">
            {a}
            <button aria-label={`Remover ${a}`} onClick={() => onChange(items.filter((x) => x !== a))}>×</button>
          </span>
        ))}
        {items.length === 0 && <span className="muted small">Nenhum.</span>}
      </div>
      <div className="row">
        <input
          className="grow"
          value={name}
          placeholder="nome do executável, ex.: app.exe"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <button onClick={add} disabled={!name.trim()}>Adicionar</button>
      </div>
    </div>
  );
}
