// Aviso de nova versão. A única rede usada é a consulta do backend à API pública de releases do GitHub (update_check).
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t as tr, tf } from "./i18n";

export type UpdateInfo = { current: string; latest: string; url: string; notes_excerpt: string; update_available: boolean; known: boolean };
const DAY = 24 * 3600 * 1000;
export const updateEnabled = () => localStorage.getItem("updateCheck") !== "0";
export const setUpdateEnabled = (on: boolean) => { localStorage.setItem("updateCheck", on ? "1" : "0"); window.dispatchEvent(new Event("coreforge-update")); };
const cached = (): UpdateInfo | null => { try { return JSON.parse(localStorage.getItem("updateInfo") ?? "null"); } catch { return null; } };

/** Consulta o GitHub (nunca lança). Sem rede nenhuma se o usuário desligou a verificação. */
export async function checkUpdate(): Promise<UpdateInfo | null> {
  if (!updateEnabled()) return null;
  const r = await invoke<UpdateInfo>("update_check").catch(() => null);
  if (r?.known) { localStorage.setItem("updateLast", String(Date.now())); localStorage.setItem("updateInfo", JSON.stringify(r)); window.dispatchEvent(new Event("coreforge-update")); }
  return r?.known ? r : null;
}

export function UpdateBanner() {
  const [, bump] = useState(0);
  useEffect(() => {
    const h = () => bump((n) => n + 1);
    window.addEventListener("coreforge-update", h);
    if (updateEnabled() && Date.now() - Number(localStorage.getItem("updateLast") ?? 0) > DAY) void checkUpdate();
    return () => window.removeEventListener("coreforge-update", h);
  }, []);
  const u = cached();
  if (!updateEnabled() || !u?.update_available || localStorage.getItem("updateDismissed") === u.latest) return null;
  return (
    <div className="toast update" role="status">
      <span>{tf("Nova versão {0} disponível. Para atualizar, baixe o novo .deb na página de versões.", u.latest)}</span>
      <span>
        <button className="btn small" onClick={() => { invoke("update_open").catch(() => {}); }}>{tr("Abrir página")}</button>
        <button onClick={() => { localStorage.setItem("updateDismissed", u.latest); bump((n) => n + 1); }} aria-label={tr("Fechar")}>×</button>
      </span>
    </div>
  );
}

export function UpdateSettings() {
  const [on, setOn] = useState(updateEnabled());
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true); const r = await checkUpdate(); setBusy(false);
    setMsg(!r ? tr("Não foi possível verificar agora.") : r.update_available ? tf("Nova versão {0} disponível.", r.latest) : tr("Você está na versão mais recente."));
  };
  return (
    <>
      <label className="dim">
        <input type="checkbox" checked={on} onChange={(e) => { setOn(e.target.checked); setUpdateEnabled(e.target.checked); setMsg(""); }} /> {tr("Verificar atualizações")}
      </label>
      <button className="btn small" disabled={!on || busy} onClick={run}>{tr("Verificar agora")}</button>
      {msg && <span className="dim" role="status">{msg}</span>}
    </>
  );
}
