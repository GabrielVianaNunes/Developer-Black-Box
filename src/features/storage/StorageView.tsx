import { useEffect, useState } from "react";
import { usePolling } from "../../app/hooks";
import { useI18n } from "../../i18n";
import { deleteActivity, getSettings, getStorage, setSettings, verifyIntegrity } from "../../services/backend";
import type { Settings, Verify } from "../../types/dashboard";

export function StorageView() {
  const { t, f, errorText } = useI18n();
  const st = usePolling(getStorage, 4000);
  const [settings, setLocal] = useState<Settings | null>(null);
  const [mb, setMb] = useState(256);
  const [hours, setHours] = useState(24);
  const [verify, setVerify] = useState<Verify | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => {
        setLocal(s);
        setMb(s.retentionMaxMb);
        setHours(s.retentionMaxHours);
      })
      .catch((e) => setMsg(errorText(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const s = st.data;
  const used = s ? Math.min(100, (s.storageBytes / Math.max(1, s.maxTotalBytes)) * 100) : 0;
  const preserved = s ? s.segments.filter((x) => x.preserved).length : 0;

  async function saveRetention() {
    if (!settings) return;
    setMsg(null);
    try {
      const next = await setSettings({ ...settings, retentionMaxMb: mb, retentionMaxHours: hours });
      setLocal(next);
      setMsg(t("storage.retentionApplied"));
      void st.reload();
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  async function remove(includePreserved: boolean) {
    if (!window.confirm(t(includePreserved ? "storage.confirmAll" : "storage.confirmActivity"))) return;
    setMsg(null);
    try {
      const n = await deleteActivity(includePreserved);
      setMsg(t("storage.deleted", { n }));
      void st.reload();
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  async function runVerify() {
    try {
      setVerify(await verifyIntegrity());
    } catch (e) {
      setMsg(errorText(e));
    }
  }

  return (
    <section aria-label={t("nav.storage")}>
      <div className="card">
        <h2>{t("storage.usedTitle")}</h2>
        {s ? (
          <>
            <div className="bar" role="progressbar" aria-valuenow={Math.round(used)} aria-valuemin={0} aria-valuemax={100}>
              <div className="bar-fill" style={{ width: `${used}%` }} />
            </div>
            <p>
              {t("storage.usedLine", {
                used: f.bytes(s.storageBytes),
                max: f.bytes(s.maxTotalBytes),
                segments: s.segments.length,
                preserved,
              })}
            </p>
          </>
        ) : (
          <p className="muted">{t("app.loading")}</p>
        )}
        <p className="muted small">{t("storage.location")}</p>
      </div>

      <div className="card">
        <h2>{t("storage.retentionTitle")}</h2>
        <div className="row">
          <label className="inline">
            {t("storage.limitMb")}
            <input type="number" min={16} max={10240} value={mb} onChange={(e) => setMb(Number(e.target.value))} />
          </label>
          <label className="inline">
            {t("storage.keepHours")}
            <input type="number" min={1} max={720} value={hours} onChange={(e) => setHours(Number(e.target.value))} />
          </label>
          <button className="primary" onClick={saveRetention}>{t("storage.apply")}</button>
        </div>
        <p className="muted small">{t("storage.retentionHelp")}</p>
      </div>

      <div className="card">
        <h2>{t("storage.integrityTitle")}</h2>
        <button onClick={runVerify}>{t("storage.verify")}</button>
        {verify && (
          <p role="status" className={verify.ok ? "ok" : "bad"}>
            {verify.ok
              ? t("storage.verifyOk", { segments: verify.segments, events: verify.events })
              : t("storage.verifyBad", { error: errorText(verify.error) })}
          </p>
        )}
        <p className="muted small">{t("storage.integrityHelp")}</p>
      </div>

      <div className="card">
        <h2>{t("storage.deleteTitle")}</h2>
        <div className="row">
          <button className="danger" onClick={() => remove(false)}>{t("storage.deleteActivity")}</button>
          <button className="danger" onClick={() => remove(true)}>{t("storage.deleteAll")}</button>
        </div>
        <p className="muted small">{t("storage.deleteHelp")}</p>
        {msg && <p className="notice" role="status">{msg}</p>}
        {st.error && <p className="notice">{errorText(st.error)}</p>}
      </div>
    </section>
  );
}
