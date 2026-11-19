// Páginas extras: Limpeza, CPU (detalhes) e Limites (estrangulamento/potência). Tudo lido do sistema; a limpeza só apaga com confirmação.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t as tr, tf } from "./i18n";

const fmt = (b: number) => (b >= 2 ** 30 ? `${(b / 2 ** 30).toFixed(1)} GB` : b >= 2 ** 20 ? `${(b / 2 ** 20).toFixed(0)} MB` : `${Math.round(b / 1024)} KB`);
const Empty = ({ text }: { text: string }) => <p className="dim">{tr(text)}</p>;

type Cat = { id: string; label: string; bytes: number; cleanable: boolean; hint: string };
export function Cleanup() {
  const [cats, setCats] = useState<Cat[] | null>(null);
  const [busy, setBusy] = useState("");
  const [msg, setMsg] = useState("");
  const scan = () => invoke<Cat[]>("cleanup_scan").then((c) => setCats(c ?? [])).catch((e) => { setCats([]); setMsg(String(e)); });
  useEffect(() => { scan(); }, []);
  const run = (c: Cat) => {
    if (busy || !window.confirm(tf("Apagar o conteúdo de “{0}” ({1})? Isso não pode ser desfeito.", tr(c.label), fmt(c.bytes)))) return;
    setBusy(c.id); setMsg("");
    invoke<number>("cleanup_run", { id: c.id }).then((n) => setMsg(tf("{0} liberados.", fmt(n)))).catch((e) => setMsg(String(e))).finally(() => { setBusy(""); scan(); });
  };
  return (
    <>
      <p className="dim">{tr("Mostra o que ocupa espaço. Só as categorias do seu usuário podem ser limpas aqui, uma por vez e com confirmação; o que exige administrador aparece com o comando para rodar no terminal.")}</p>
      {msg && <p role="status">{tr(msg)}</p>}
      <section className="card">
        {cats === null && <Empty text="Calculando tamanhos…" />}
        {cats?.length === 0 && <Empty text="Não foi possível ler as categorias." />}
        {cats?.map((c) => (
          <div className="kv" key={c.id}>
            <span><b>{tr(c.label)}</b>{c.hint && <><br /><span className="dim">{tr(c.hint)}</span></>}</span>
            <span className="row" style={{ gap: 12, alignItems: "center", flexWrap: "nowrap" }}>
              {(c.cleanable || c.bytes > 0) && <span className="strong">{fmt(c.bytes)}</span>}
              {c.cleanable && <button className="btn small danger" disabled={!!busy || c.bytes === 0} onClick={() => run(c)}>{busy === c.id ? tr("Limpando…") : tr("Limpar agora")}</button>}
            </span>
          </div>
        ))}
      </section>
      <button className="btn small ghost" onClick={scan} disabled={!!busy}>{tr("Recalcular")}</button>
    </>
  );
}

type Details = { lscpu: [string, string][]; cores: { id: number; cur_mhz: number | null; max_mhz: number | null; governor: string }[]; driver: string; boost: boolean | null; flags: string[] };
export function CpuDetails() {
  const [d, setD] = useState<Details | null>(null);
  const [live, setLive] = useState(0);
  useEffect(() => { invoke<Details>("cpu_details").then(setD).catch(() => setD({ lscpu: [], cores: [], driver: "", boost: null, flags: [] })); }, [live]);
  useEffect(() => { const i = window.setInterval(() => setLive((n) => n + 1), 2000); return () => window.clearInterval(i); }, []);
  if (!d) return <Empty text="Lendo a CPU…" />;
  const govs = [...new Set(d.cores.map((c) => c.governor).filter(Boolean))].join(", ");
  const kv = (k: string, v: string) => <div className="kv" key={k}><span className="dim">{k}</span><span className="strong">{v}</span></div>;
  return (
    <>
      <section className="card">
        {d.lscpu.length === 0 && <Empty text="O comando lscpu não está disponível." />}
        {d.lscpu.map(([k, v]) => kv(k, v))}
      </section>
      <section className="card">
        <h2>{tr("Frequência")}</h2>
        {d.cores.every((c) => c.cur_mhz == null) ? <Empty text="O kernel não expõe a frequência (cpufreq) neste PC." /> : <>
          {kv(tr("Governador"), govs || "—")}{kv(tr("Driver"), d.driver || "—")}
          {kv(tr("Boost / turbo"), d.boost == null ? "—" : d.boost ? tr("Ligado") : tr("Desligado"))}
          <div className="grid2">{d.cores.map((c) => kv(`CPU ${c.id}`, c.cur_mhz == null ? "—" : `${c.cur_mhz} / ${c.max_mhz ?? "?"} MHz`))}</div>
        </>}
      </section>
      <section className="card">
        <h2>{tr("Instruções")} ({d.flags.length})</h2>
        {d.flags.length ? <p className="dim" style={{ wordBreak: "break-word" }}>{d.flags.join(" ")}</p> : <Empty text="Sem informações de instruções." />}
      </section>
    </>
  );
}

type Finding = { label: string; value: string; level: "ok" | "warn" | "info" };
export function Throttle() {
  const [f, setF] = useState<Finding[] | null>(null);
  const [gpu, setGpu] = useState<string | null>(null);
  const load = () => {
    invoke<Finding[]>("throttle_report").then((x) => setF(x ?? [])).catch(() => setF([]));
    invoke<{ gpu_throttle: string | null }>("live_metrics").then((m) => setGpu(m?.gpu_throttle ?? null)).catch(() => {});
  };
  useEffect(load, []);
  if (!f) return <Empty text="Verificando…" />;
  const mark = { ok: "✓", warn: "!", info: "i" } as const;
  return (
    <>
      <p className="dim">{tr("Leitura somente: mostra se o kernel está limitando a CPU ou a GPU por temperatura ou potência. Nada é alterado.")}</p>
      <section className="card">
        {f.map((x) => <div className="kv" key={x.label}><span>{mark[x.level]} {tr(x.label)}</span><span className={x.level === "warn" ? "reason" : "strong"} style={{ margin: 0 }}>{tr(x.value)}</span></div>)}
        <div className="kv"><span>{gpu ? "!" : "✓"} {tr("Limitador da GPU (NVIDIA)")}</span><span className="strong">{gpu ?? tr("Nenhum")}</span></div>
      </section>
      <button className="btn small ghost" onClick={load}>{tr("Verificar de novo")}</button>
    </>
  );
}
