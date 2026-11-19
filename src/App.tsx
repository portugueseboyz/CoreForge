import { t as tr, tf, lang, setLang } from "./i18n";
import { Component, Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import { setInterval } from "./poll";
import { pushSample } from "./history";
import { loadRuns, saveRun, RUN_KINDS, type Run } from "./runs";
import { buildReport } from "./report";
import { tempLimit, curveX as x, curveY as y, extendCurve } from "./curvemath";
import { History } from "./History";
import { Cleanup, CpuDetails, Throttle } from "./Extras";
import { Chart, TEMP_LINES, LOAD_LINES, POWER_LINES, BOTTLENECK_LINES } from "./Chart";
import { invoke } from "@tauri-apps/api/core";
import logo from "./assets/logo.svg";
import "./App.css";

type InfoNode = { header: string; value: string; children: InfoNode[] };
type Sensor = { id: string; hardware: string; category: string; name: string; value: number; unit: string };
type Range = { min: number; max: number };


function Node({ n, depth = 0 }: { n: InfoNode; depth?: number }) {
  const [open, setOpen] = useState(depth < 1 || depth === 0);
  const group = n.children.length > 0;
  return (
    <div>
      <div className={group ? "node group" : "node"} style={{ paddingLeft: depth * 18 }} onClick={() => group && setOpen(!open)}>
        <span className="caret">{group ? (open ? "▾" : "▸") : ""}</span>
        <span className="hdr">{tr(n.header)}</span>
        {n.value && <span className="val">{tr(n.value)}</span>}
      </div>
      {group && open && n.children.map((c, i) => <Node key={i} n={c} depth={depth + 1} />)}
    </div>
  );
}

// Linha que vira destaque no topo do cartão (o dado que a pessoa mais procura).
const HERO_KEYS = ["Distribuição", "Modelo", "Total"];

function InfoCard({ n }: { n: InfoNode }) {
  const leaves = n.children.filter((c) => c.children.length === 0);
  const groups = n.children.filter((c) => c.children.length > 0);
  const hero = leaves.find((c) => HERO_KEYS.includes(c.header));
  const kv = (c: InfoNode, i: number) => <div className="kv" key={i}><span className="dim">{tr(c.header)}</span><span className="strong">{tr(c.value) || "—"}</span></div>;
  return (
    <section className="card info-card">
      <h2>{tr(n.header)}</h2>
      {hero && <div className="hero">{tr(hero.value)}<small>{tr(hero.header)}</small></div>}
      {leaves.filter((c) => c !== hero).map(kv)}
      {groups.map((g, i) => (
        <div className="sub-block" key={i}>
          <div className="sub-title">{tr(g.header)}</div>
          {g.children.map(kv)}
        </div>
      ))}
    </section>
  );
}

function Info() {
  const [nodes, setNodes] = useState<InfoNode[]>([]);
  useEffect(() => { invoke<InfoNode[]>("system_info").then((x) => setNodes(x ?? [])); }, []);
  // Ordem pensada para a grade de 3 colunas: cada linha reúne cartões de tamanho parecido.
  const order = ["Sistema operacional", "Processador", "Placa-mãe e BIOS", "Memória", "Placa de vídeo", "Armazenamento"];
  const rank = (n: InfoNode) => { const i = order.indexOf(n.header); return i < 0 ? order.length : i; };
  const [report, setReport] = useState("");
  const share = () => { const t = buildReport(nodes); setReport(t); navigator.clipboard?.writeText(t).catch(() => {}); };
  const [saved, setSaved] = useState("");
  const save = async () => {
    const [sensors, checks] = await Promise.all([invoke<Sensor[]>("sensors"), invoke<Check[]>("system_check")]);
    const t = buildReport(nodes, "0.1.0", { sensors, checks });
    setReport(t);
    invoke<string>("report_save", { text: t }).then((p) => setSaved(tf("Relatório salvo em {0}", p)), (e) => setSaved(tf("Não foi possível salvar: {0}", String(e))));
  };
  return (
    <>
      <div className="cards">{[...nodes].sort((a, b) => rank(a) - rank(b)).map((n, i) => <InfoCard key={i} n={n} />)}</div>
      <div className="row" style={{ marginTop: 18 }}><button className="btn small" onClick={share}>{tr("Copiar relatório")}</button>
        <button className="btn small" onClick={save}>{tr("Salvar relatório")}</button>{saved && <span className="dim" role="status">{saved}</span>}</div>
      {report && <section className="card" style={{ marginTop: 14 }}><p className="dim">{tr("Copiado para a área de transferência. Cole onde precisar (fórum, pedido de suporte):")}</p><pre className="log">{report}</pre></section>}
    </>
  );
}

type GpuLive = { index: number; items: [string, string][] };

function Gpu() {
  const [info, setInfo] = useState<InfoNode[]>([]);
  const [live, setLive] = useState<GpuLive[]>([]);
  useEffect(() => {
    invoke<InfoNode[]>("gpu_info").then((x) => setInfo(x ?? []));
    let alive = true;
    const tick = () => invoke<GpuLive[]>("gpu_live").then((l) => alive && setLive(l ?? []));
    tick();
    const t = setInterval(tick, 1000);
    return () => { alive = false; clearInterval(t); };
  }, []);
  if (info.length === 0) return <p className="dim">{tr("Nenhuma placa de vídeo com leitura disponível: NVIDIA sem o driver proprietário (NVML), ou AMD/Intel sem dados no sysfs.")}</p>;
  return (
    <>
      {info.map((g, i) => (
        <section key={i} className="card">
          <h2>{g.header}</h2>
          <div className="cols">
            <div>{g.children.map((c, j) => <Node key={j} n={c} depth={0} />)}</div>
            <div className="live">
              <h3>{tr("Ao vivo")}</h3>
              {(live[i]?.items ?? []).map(([k, v]) => (
                <div key={k} className="kv"><span className="dim">{tr(k)}</span><span className="strong">{tr(v)}</span></div>
              ))}
            </div>
          </div>
        </section>
      ))}
    </>
  );
}

type StressStatus = { running: boolean; elapsed_s: number; temp: number | null; max_temp: number | null;
  mem_errors: number; mem_passes: number; mem_gb: number; reason: string };

function Stress() {
  const [cpu, setCpu] = useState(true);
  const [ram, setRam] = useState(false);
  const [frac, setFrac] = useState(60);
  const [limit, setLimit] = useState(90);
  const [minutes, setMinutes] = useState(5);
  const [st, setSt] = useState<StressStatus | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    const t = setInterval(() => invoke<StressStatus | null>("stress_status").then(setSt), 500);
    return () => clearInterval(t);
  }, []);
  const running = st?.running ?? false;
  const start = () => invoke("stress_start", { cpu, ram, ramFraction: frac / 100, tempLimit: limit, minutes })
    .then(() => setErr("")).catch((e) => setErr(String(e)));
  const t = (v: number | null) => (v == null ? "—" : `${v.toFixed(0)} °C`);
  return (
    <>
      <section className="card form">
        <label><input type="checkbox" checked={cpu} disabled={running} onChange={(e) => setCpu(e.target.checked)} /> {tr("CPU (todos os threads)")}</label>
        <label><input type="checkbox" checked={ram} disabled={running} onChange={(e) => setRam(e.target.checked)} /> {tr("RAM (grava e verifica padrões)")}</label>
        <label>{tf("Memória livre usada: {0} %", frac)}<input type="range" min={10} max={90} value={frac} disabled={running} onChange={(e) => setFrac(+e.target.value)} /></label>
        <label>{tr("Parar na temperatura (°C, 0 = sem limite)")}<input type="number" value={limit} disabled={running} onChange={(e) => setLimit(+e.target.value)} /></label>
        <label>{tr("Duração (min, 0 = até parar)")}<input type="number" value={minutes} disabled={running} onChange={(e) => setMinutes(+e.target.value)} /></label>
        <div>
          {running ? <button className="btn danger" onClick={() => invoke("stress_stop")}>{tr("Parar")}</button>
                   : <button className="btn" onClick={start}>{tr("Iniciar")}</button>}
          {err && <span className="err"> {err}</span>}
        </div>
      </section>
      {st && (
        <section className="card">
          <div className="kv"><span className="dim">{tr("Estado")}</span><span className="strong">{running ? "Rodando" : "Parado"}{st.reason && ` · ${st.reason}`}</span></div>
          <div className="kv"><span className="dim">{tr("Tempo")}</span><span className="strong">{st.elapsed_s.toFixed(0)} s</span></div>
          <div className="kv"><span className="dim">{tr("Temperatura da CPU (atual / máxima)")}</span><span className="strong">{t(st.temp)} / {t(st.max_temp)}</span></div>
          <div className="kv"><span className="dim">{tr("RAM testada")}</span><span className="strong">{st.mem_gb.toFixed(1)} GB · {st.mem_passes} passadas</span></div>
          <div className="kv"><span className="dim">{tr("Erros de memória")}</span><span className={st.mem_errors ? "strong err" : "strong"}>{st.mem_errors}</span></div>
        </section>
      )}
      <div className="cards cards-2">
        <Chart title={tr("Temperaturas")} lines={TEMP_LINES.slice(0, 1)} unit="°C" />
        <Chart title={tr("Carga")} lines={LOAD_LINES.slice(0, 1).concat(LOAD_LINES.slice(2, 3))} unit="%" min={0} max={100} />
      </div>
    </>
  );
}

type BenchStatus = { running: boolean; progress: number; passes: number; elapsed_s: number;
  result: { threads: number; seconds: number; samples: number; passes: number; score: number } | null };
const BW = 960, BH = 540;

/** Cartão com os últimos resultados de um benchmark e a diferença para o anterior. Também guarda o resultado novo. */
function RunHistory({ kind, current }: { kind: string; current: { sig: string; value: number; detail: string } | null }) {
  const [runs, setRuns] = useState<Run[]>(() => loadRuns(kind));
  useEffect(() => { if (current) setRuns(saveRun(kind, current)); }, [current?.sig]);
  if (runs.length === 0) return null;
  const unit = RUN_KINDS[kind].unit;
  return (
    <section className="card">
      <h3 className="mini">{tr("Histórico")}</h3>
      {runs.map((r, i) => {
        const prev = runs[i + 1];
        const d = prev ? ((r.value - prev.value) / prev.value) * 100 : null;
        return (
          <div className="kv" key={r.at}>
            <span className="dim">{new Date(r.at).toLocaleString()} · {r.detail}</span>
            <span className="strong">{Math.round(r.value)} {unit}{d != null && Math.abs(d) >= 0.5 && <span className={d > 0 ? "lv-ok" : "lv-bad"}> {d > 0 ? "▲" : "▼"} {Math.abs(d).toFixed(1)} %</span>}</span>
          </div>
        );
      })}
      <button className="btn small danger" onClick={() => { localStorage.removeItem(`runs:${kind}`); setRuns([]); }}>{tr("Limpar histórico")}</button>
    </section>
  );
}

type FanSet = { kind: string; label: string; percent: number | null };
type LedSet = { device: string; zone: string | null; mode: string | null; color: string | null; colors: string[] | null };
type Profile = { name: string; fans: FanSet[]; leds: LedSet[]; apply_on_start: boolean };

/** Perfis: guardam o que o app ajustou (ventoinhas e LEDs) e o reaplicam com um clique ou ao iniciar a sessão. */
function Profiles() {
  const [list, setList] = useState<Profile[]>([]);
  const [draft, setDraft] = useState<{ fans: FanSet[]; leds: LedSet[] } | null>(null);
  const [name, setName] = useState("");
  const [auto, setAuto] = useState(false);
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState("");
  const load = () => {
    invoke<Profile[]>("profiles_list").then(setList);
    invoke<{ fans: FanSet[]; leds: LedSet[] }>("profile_capture").then(setDraft);
    invoke<boolean>("profile_autostart_get").then(setAuto);
  };
  useEffect(load, []);
  const save = () => invoke("profile_save", { name }).then(() => { setName(""); setMsg(tr("Perfil salvo.")); load(); }).catch((e) => setMsg(tr(String(e))));
  const apply = (p: Profile) => {
    if (p.fans.length > 0 && !window.confirm(tf("Aplicar o perfil {0}? As ventoinhas mudam de velocidade e o sistema vai pedir a sua senha.", p.name))) return;
    setBusy(p.name);
    invoke<string>("profile_apply", { name: p.name }).then((m) => setMsg(tf("{0}: aplicado ({1})", p.name, m))).catch((e) => setMsg(tr(String(e)))).finally(() => { setBusy(""); load(); });
  };
  const remove = (p: Profile) => { if (window.confirm(tf("Excluir o perfil {0}?", p.name))) invoke("profile_delete", { name: p.name }).then(load); };
  const start = (p: Profile) => invoke("profile_set_start", { name: p.apply_on_start ? null : p.name }).then(load);
  const toggleAuto = (on: boolean) => invoke("profile_autostart_set", { enabled: on }).then(() => setAuto(on)).catch((e) => setMsg(String(e)));
  const empty = !draft || (draft.fans.length === 0 && draft.leds.length === 0);
  return (
    <>
      <section className="card form">
        <h3 className="mini">{tr("Salvar o estado atual")}</h3>
        <p className="dim">{draft ? tf("O perfil guarda {0} ventoinha(s) em velocidade manual e {1} ajuste(s) de LED feitos por aqui.", draft.fans.length, draft.leds.length) : "…"} {empty && tr("Ajuste uma ventoinha ou um LED nas outras abas e volte para salvar.")}</p>
        <div className="row filters">
          <input type="text" value={name} maxLength={40} onChange={(e) => setName(e.target.value)} placeholder={tr("Nome do perfil")} aria-label={tr("Nome do perfil")} />
          <button className="btn small" disabled={empty || !name.trim()} onClick={save}>{tr("Salvar perfil")}</button>
        </div>
        <label className="check"><input type="checkbox" checked={auto} onChange={(e) => toggleAuto(e.target.checked)} /> {tr("Reaplicar as luzes do perfil marcado ao iniciar a sessão (sem abrir janela e sem senha). As ventoinhas só mudam quando você aplica o perfil aqui, porque pedem a senha do sistema.")}</label>
      </section>
      {msg && <p className="dim">{msg}</p>}
      {list.length === 0 && <p className="dim">{tr("Nenhum perfil salvo ainda.")}</p>}
      <div className="cards cards-2">
        {list.map((p) => (
          <section className="card" key={p.name}>
            <div className="fan-head"><b>{p.name}</b>{p.apply_on_start && <span className="badge good">{tr("ao iniciar")}</span>}</div>
            {p.fans.map((f, i) => <div className="kv" key={`f${i}`}><span className="dim">{tr(f.label)}</span><span className="strong">{f.percent == null ? tr("Automático") : `${f.percent} %`}</span></div>)}
            {p.leds.map((l, i) => (
              <div className="kv" key={`l${i}`}><span className="dim">{l.device}{l.zone ? ` · ${l.zone}` : ""}</span>
                <span className="strong">{l.colors?.length ? `${l.colors.length} ${tr("cores")}` : l.color ? <><i className="swatch" style={{ background: `#${l.color.replace("#", "")}` }} /> {l.mode ?? ""}</> : (l.mode ?? "—")}</span></div>
            ))}
            <div className="row fan-ctl">
              <button className="btn small" disabled={busy === p.name} onClick={() => apply(p)}>{tr("Aplicar")}</button>
              <button className="btn small ghost" onClick={() => start(p)}>{p.apply_on_start ? tr("Não usar ao iniciar") : tr("Usar ao iniciar")}</button>
              <button className="btn small danger" onClick={() => remove(p)}>{tr("Excluir")}</button>
            </div>
          </section>
        ))}
      </div>
    </>
  );
}

function Bench() {
  const [multi, setMulti] = useState(true);
  const [minutes, setMinutes] = useState(0);
  const [st, setSt] = useState<BenchStatus | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const draw = async () => {
    const buf = await invoke<ArrayBuffer>("bench_image");
    const g = new Uint8Array(buf);
    const c = canvas.current?.getContext("2d");
    if (!c || g.length !== BW * BH) return;
    const img = c.createImageData(BW, BH);
    for (let i = 0; i < g.length; i++) { img.data[i * 4] = img.data[i * 4 + 1] = img.data[i * 4 + 2] = g[i]; img.data[i * 4 + 3] = 255; }
    c.putImageData(img, 0, 0);
  };
  useEffect(() => {
    let was = false;
    const t = setInterval(async () => {
      const s = await invoke<BenchStatus>("bench_status");
      setSt(s);
      if (s.running || was) await draw(); // uma última leitura ao terminar
      was = s.running;
    }, 700);
    return () => clearInterval(t);
  }, []);
  const running = st?.running ?? false;
  return (
    <>
      <section className="card form">
        <label><input type="radio" checked={multi} disabled={running} onChange={() => setMulti(true)} /> {tr("Multi-core (todas as threads)")}</label>
        <label><input type="radio" checked={!multi} disabled={running} onChange={() => setMulti(false)} /> {tr("Single-core (1 thread)")}</label>
        <label>{tr("Duração mínima (min, 0 = uma passada)")}<input type="number" value={minutes} disabled={running} onChange={(e) => setMinutes(+e.target.value)} /></label>
        <div>
          {running ? <button className="btn danger" onClick={() => invoke("bench_cancel")}>{tr("Cancelar")}</button>
                   : <button className="btn" onClick={() => invoke("bench_start", { multi, minMinutes: minutes })}>{tr("Iniciar")}</button>}
        </div>
      </section>
      <section className="card">
        {running && <div className="bar"><div style={{ width: `${(st?.progress ?? 0) * 100}%` }} /></div>}
        {st?.result && (
          <div className="score">{st.result.score.toFixed(0)} <small>pts · {st.result.seconds.toFixed(1)} s · {st.result.threads} thread(s) · {st.result.passes} passada(s)</small></div>
        )}
        <canvas ref={canvas} width={BW} height={BH} className="bench" />
      </section>
      <RunHistory kind="bench-cpu" current={st?.result ? { sig: JSON.stringify(st.result), value: st.result.score, detail: tf("{0} thread(s)", st.result.threads) } : null} />
    </>
  );
}

type Mount = { path: string; source: string; size_gb: number; free_gb: number };
type DiskResult = { mount: string; file_mb: number; direct: boolean; seq_read_mbs: number; seq_write_mbs: number;
  read_iops_q1: number; write_iops_q1: number; read_latency_us: number; write_latency_us: number; read_iops_q8: number };
type DiskStatus = { running: boolean; phase: string; progress: number; result: DiskResult | null; error: string };
type Health = { name: string; meta: string; serial: string; verdict: "good" | "warning" | "critical" | "unknown";
  reasons: string[]; metrics: [string, string][] };
const VERDICT: Record<Health["verdict"], string> = { good: "SAUDÁVEL", warning: "ATENÇÃO", critical: "CRÍTICO", unknown: "SEM DADOS" };

function Disk() {
  const [mounts, setMounts] = useState<Mount[]>([]);
  const [mount, setMount] = useState("");
  const [mb, setMb] = useState(1024);
  const [st, setSt] = useState<DiskStatus | null>(null);
  const [health, setHealth] = useState<Health[]>([]);
  useEffect(() => {
    invoke<Mount[]>("disk_mounts").then((m0) => { const m = m0 ?? []; setMounts(m); setMount((c) => c || m[0]?.path || ""); });
    invoke<Health[]>("disk_health").then((x) => setHealth(x ?? []));
    const t = setInterval(() => invoke<DiskStatus>("disk_bench_status").then(setSt), 500);
    return () => clearInterval(t);
  }, []);
  const running = st?.running ?? false;
  const r = st?.result;
  const row = (k: string, v: string) => <div className="kv" key={k}><span className="dim">{k}</span><span className="strong">{v}</span></div>;
  return (
    <>
      <h2 className="sub">{tr("Saúde")}</h2>
      {health.map((h, i) => (
        <section key={i} className="card">
          <div className="head"><b>{h.name}</b><span className={`badge ${h.verdict}`}>{tr(VERDICT[h.verdict])}</span></div>
          <div className="dim">{tr(h.meta)}{h.serial && ` · ${h.serial}`}</div>
          {h.metrics.map(([k, v]) => row(tr(k), tr(v)))}
          {h.reasons.map((x, j) => <p key={j} className="reason">{tr(x)}</p>)}
        </section>
      ))}
      <h2 className="sub">{tr("Benchmark")}</h2>
      <section className="card form">
        <label>Unidade
          <select value={mount} disabled={running} onChange={(e) => setMount(e.target.value)}>
            {mounts.map((m) => <option key={m.path} value={m.path}>{m.path} · {m.source} · {m.free_gb.toFixed(0)} {tr("GB livres")}</option>)}
          </select>
        </label>
        <label>{tr("Tamanho do arquivo de teste")}
          <select value={mb} disabled={running} onChange={(e) => setMb(+e.target.value)}>
            {[256, 1024, 4096].map((v) => <option key={v} value={v}>{v >= 1024 ? `${v / 1024} GB` : `${v} MB`}</option>)}
          </select>
        </label>
        <div>
          {running ? <button className="btn danger" onClick={() => invoke("disk_bench_cancel")}>{tr("Cancelar")}</button>
                   : <button className="btn" disabled={!mount} onClick={() => invoke("disk_bench_start", { mount, fileMb: mb })}>{tr("Iniciar")}</button>}
        </div>
      </section>
      {st && (running || r || st.error) && (
        <section className="card">
          {running && <><div className="bar"><div style={{ width: `${st.progress * 100}%` }} /></div><div className="dim">{tr(st.phase)}…</div></>}
          {st.error && <p className="err">{tr(st.error)}</p>}
          {r && <>
            {row(tr("Leitura sequencial (1 MB)"), `${r.seq_read_mbs.toFixed(0)} MB/s`)}
            {row(tr("Escrita sequencial (1 MB)"), `${r.seq_write_mbs.toFixed(0)} MB/s`)}
            {row(tr("Leitura 4K Q1"), `${r.read_iops_q1.toFixed(0)} IOPS · ${r.read_latency_us.toFixed(0)} µs`)}
            {row(tr("Escrita 4K Q1"), `${r.write_iops_q1.toFixed(0)} IOPS · ${r.write_latency_us.toFixed(0)} µs`)}
            {row(tr("Leitura 4K Q8"), `${r.read_iops_q8.toFixed(0)} IOPS`)}
            {!r.direct && <p className="reason">{tr("Este sistema de arquivos não aceita E/S direta; os números incluem o cache do sistema.")}</p>}
          </>}
        </section>
      )}
      <RunHistory kind="disk" current={r ? { sig: JSON.stringify(r), value: r.seq_read_mbs, detail: `${tr("Escrita sequencial (1 MB)")} ${r.seq_write_mbs.toFixed(0)} MB/s · ${r.mount}` } : null} />
    </>
  );
}

type NetRow = { label: string; value: string; level: "ok" | "warn" | "bad" | "info" };
type NetStatus = { running: boolean; phase: string; progress: number; rows: NetRow[] };
type Iface = { name: string; kind: string; up: boolean; speed: string; mac: string; addrs: string[] };

function Net() {
  const [st, setSt] = useState<NetStatus | null>(null);
  const [ifs, setIfs] = useState<Iface[]>([]);
  useEffect(() => {
    invoke<Iface[]>("net_interfaces").then((x) => setIfs(x ?? []));
    const t = setInterval(() => invoke<NetStatus>("net_status").then(setSt), 500);
    return () => clearInterval(t);
  }, []);
  const running = st?.running ?? false;
  return (
    <>
      <section className="card">
        {ifs.map((i) => (
          <div className="kv" key={i.name}>
            <span><b>{i.name}</b> <span className="dim">{i.kind} · {i.up ? "conectado" : "desconectado"} · {i.speed}</span></span>
            <span className="strong">{i.addrs.join(", ") || "—"}</span>
          </div>
        ))}
      </section>
      <section className="card form">
        <p className="dim">{tr("Mede latência, jitter e perda até o roteador e a internet, DNS, velocidade e latência sob carga (usa speed.cloudflare.com, ~16 s de tráfego).")}</p>
        <div>
          {running ? <button className="btn danger" onClick={() => invoke("net_cancel")}>{tr("Cancelar")}</button>
                   : <button className="btn" onClick={() => invoke("net_start")}>{tr("Iniciar teste")}</button>}
        </div>
      </section>
      {st && st.rows.length + (running ? 1 : 0) > 0 && (
        <section className="card">
          {running && <><div className="bar"><div style={{ width: `${st.progress * 100}%` }} /></div><div className="dim">{tr(st.phase)}…</div></>}
          {st.rows.map((r, i) => (
            <div className="kv" key={i}><span className="dim">{tr(r.label)}</span><span className={`strong lv-${r.level}`}>{tr(r.value)}</span></div>
          ))}
        </section>
      )}
    </>
  );
}

type GpuStressStatus = { running: boolean; adapter: string; elapsed_s: number; dispatches: number; gflops: number;
  temp: number | null; max_temp: number | null; power_w: number | null; max_power_w: number | null; util: number | null;
  reason: string; error: string };

function GpuStress() {
  const [intensity, setIntensity] = useState(2);
  const [limit, setLimit] = useState(85);
  const [minutes, setMinutes] = useState(2);
  const [st, setSt] = useState<GpuStressStatus | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    const t = setInterval(() => invoke<GpuStressStatus>("gpu_stress_status").then(setSt), 500);
    return () => clearInterval(t);
  }, []);
  const running = st?.running ?? false;
  const v = (x: number | null, u: string, d = 0) => (x == null ? "—" : `${x.toFixed(d)} ${u}`);
  const row = (k: string, val: string, cls = "") => <div className="kv" key={k}><span className="dim">{k}</span><span className={`strong ${cls}`}>{val}</span></div>;
  return (
    <>
      <section className="card form">
        <p className="dim">{tr("Carga máxima por compute shader (Vulkan) em todos os núcleos da GPU, sem janela. Pode esquentar e fazer as ventoinhas subirem; o teste para sozinho no limite de temperatura.")}</p>
        <label>Intensidade
          <select value={intensity} disabled={running} onChange={(e) => setIntensity(+e.target.value)}>
            <option value={1}>{tr("Normal (~80 % de ocupação)")}</option><option value={2}>{tr("Alta")}</option><option value={3}>{tr("Extrema (fila dupla)")}</option>
          </select>
        </label>
        <label>{tr("Parar na temperatura (°C, 0 = sem limite)")}<input type="number" value={limit} disabled={running} onChange={(e) => setLimit(+e.target.value)} /></label>
        <label>{tr("Duração (min, 0 = até parar)")}<input type="number" value={minutes} disabled={running} onChange={(e) => setMinutes(+e.target.value)} /></label>
        <div>
          {running ? <button className="btn danger" onClick={() => invoke("gpu_stress_stop")}>{tr("Parar")}</button>
                   : <button className="btn" onClick={() => invoke("gpu_stress_start", { intensity, tempLimit: limit, minutes }).then(() => setErr("")).catch((e) => setErr(String(e)))}>{tr("Iniciar")}</button>}
          {err && <span className="err"> {err}</span>}
        </div>
      </section>
      {st && (st.running || st.elapsed_s > 0 || st.error) && (
        <section className="card">
          {st.error && <p className="err">{tr(st.error)}</p>}
          {row("Estado", `${running ? tr("Rodando") : tr("Parado")}${st.reason ? ` · ${tr(st.reason)}` : ""}`)}
          {row("Placa", st.adapter || "—")}
          {row("Tempo", tf("{0} s · {1} execuções", st.elapsed_s.toFixed(0), st.dispatches))}
          {row(tr("Desempenho (estimado)"), `${st.gflops.toFixed(0)} GFLOPS`)}
          {row(tr("Uso da GPU"), v(st.util, "%"))}
          {row(tr("Temperatura (atual / máxima)"), `${v(st.temp, "°C")} / ${v(st.max_temp, "°C")}`)}
          {row(tr("Consumo (atual / máximo)"), `${v(st.power_w, "W", 1)} / ${v(st.max_power_w, "W", 1)}`)}
        </section>
      )}
      <div className="cards cards-2">
        <Chart title={tr("Temperaturas")} lines={TEMP_LINES.slice(1)} unit="°C" />
        <Chart title={tr("Consumo da GPU")} lines={POWER_LINES} unit="W" min={0} />
      </div>
    </>
  );
}

type Metrics = { gpu_name: string | null; gpu_util: number | null; gpu_temp: number | null; gpu_clock: number | null; gpu_mem_clock: number | null;
  gpu_power_w: number | null; gpu_fan: number | null; fan_rpm: number | null; vram_used_mb: number | null; vram_total_mb: number | null; gpu_throttle: string | null;
  cpu_load: number | null; cpu_busiest: number | null; cpu_temp: number | null; cpu_clock_mhz: number | null; ram_used_gb: number | null; ram_total_gb: number | null; fps: number | null; fps_stale: boolean };

const n0 = (v: number | null | undefined, u: string, d = 0) => (v == null ? "—" : `${v.toFixed(d)} ${u}`);
// Itens do overlay: chave, rótulo, formatador. (FPS só vem do log do MangoHud, se ativo; ver a página Overlay.)
const OVERLAY_ITEMS: [string, string, (m: Metrics) => string][] = [
  ["gpu_util", "GPU uso", (m) => n0(m.gpu_util, "%")], ["gpu_temp", "GPU temperatura", (m) => n0(m.gpu_temp, "°C")],
  ["gpu_clock", "GPU clock", (m) => n0(m.gpu_clock, "MHz")], ["gpu_mem_clock", "VRAM clock", (m) => n0(m.gpu_mem_clock, "MHz")],
  ["gpu_power", "GPU consumo", (m) => n0(m.gpu_power_w, "W")], ["gpu_fan", "GPU ventoinha", (m) => n0(m.gpu_fan, "%")],
  ["vram", "VRAM", (m) => (m.vram_used_mb == null ? "—" : `${(m.vram_used_mb / 1024).toFixed(1)} / ${((m.vram_total_mb ?? 0) / 1024).toFixed(0)} GB`)],
  ["cpu_load", tr("CPU uso"), (m) => n0(m.cpu_load, "%")], ["cpu_busiest", tr("CPU núcleo mais ocupado"), (m) => n0(m.cpu_busiest, "%")],
  ["cpu_temp", tr("CPU temperatura"), (m) => n0(m.cpu_temp, "°C")], ["cpu_clock", tr("CPU clock médio"), (m) => n0(m.cpu_clock_mhz, "MHz")],
  ["fps", "FPS (MangoHud)", (m) => n0(m.fps, "") + (m.fps != null && m.fps_stale ? " ~" : "")],
  ["ram", "RAM", (m) => (m.ram_used_gb == null ? "—" : `${m.ram_used_gb.toFixed(1)} / ${(m.ram_total_gb ?? 0).toFixed(0)} GB`)],
];
const DEFAULT_ITEMS = ["gpu_util", "gpu_temp", "vram", "cpu_load", "cpu_temp", "ram"];
const loadItems = (): string[] => { try { return JSON.parse(localStorage.getItem("overlayItems") ?? "") as string[]; } catch { return DEFAULT_ITEMS; } };

// Um único poll de live_metrics por janela, compartilhado por todos os componentes (o mais rápido pedido define o ritmo).
const mSubs = new Map<symbol, { ms: number; set: (m: Metrics) => void }>();
let mLast: Metrics | null = null, mTimer = 0, mEvery = 0;
function mStart() {
  const every = Math.min(...[...mSubs.values()].map((x) => x.ms));
  if (mTimer && every === mEvery) return;
  clearInterval(mTimer); mEvery = every;
  const tick = () => invoke<Metrics>("live_metrics").then((x) => { mLast = x; mSubs.forEach((s) => s.set(x)); }).catch(() => {});
  tick(); mTimer = setInterval(tick, every);
}
function useMetrics(ms = 1000) {
  const [m, setM] = useState<Metrics | null>(mLast);
  useEffect(() => {
    const id = Symbol();
    mSubs.set(id, { ms, set: setM }); mStart();
    return () => { mSubs.delete(id); if (mSubs.size === 0) { clearInterval(mTimer); mTimer = 0; mEvery = 0; } else mStart(); };
  }, [ms]);
  return m;
}

/** Estado de carregamento das páginas: avisa que está lendo e o que fazer se não terminar. */
const Loading = () => <p className="dim" role="status">{tr("Carregando… Se não terminar em alguns segundos, troque de aba e volte, ou veja a Preparação.")}</p>;

function OverlayView() {
  const m = useMetrics(1000);
  const [items, setItems] = useState<string[]>(loadItems());
  useEffect(() => { const t = setInterval(() => setItems(loadItems()), 1000); return () => clearInterval(t); }, []);
  useEffect(() => { document.body.style.background = "transparent"; document.documentElement.style.background = "transparent"; }, []);
  if (!m) return null;
  return (
    <div className="ovl" style={{ zoom: Number(localStorage.getItem("overlayScale") ?? 1) }}>
      {OVERLAY_ITEMS.filter(([k]) => items.includes(k)).map(([k, label, f]) => (
        <div key={k}><span>{tr(label)}</span><b>{f(m)}</b></div>
      ))}
    </div>
  );
}

function OverlaySettings() {
  const [items, setItems] = useState<string[]>(loadItems());
  const [corner, setCorner] = useState(localStorage.getItem("overlayCorner") ?? "TL");
  const [scale, setScale] = useState(Number(localStorage.getItem("overlayScale") ?? 1));
  const [open, setOpen] = useState(false);
  const [err, setErr] = useState("");
  const [hint, setHint] = useState("");
  useEffect(() => { invoke<string>("mangohud_hint").then(setHint).catch(() => {}); }, []);
  const toggle = (k: string) => {
    const next = items.includes(k) ? items.filter((x) => x !== k) : [...items, k];
    setItems(next); localStorage.setItem("overlayItems", JSON.stringify(next));
  };
  const save = (c: string, s: number) => { setCorner(c); setScale(s); localStorage.setItem("overlayCorner", c); localStorage.setItem("overlayScale", String(s)); };
  const go = () => invoke<boolean>("overlay_toggle", { corner, width: 260 * scale, height: (items.length * 22 + 20) * scale })
    .then((o) => { setOpen(o); setErr(""); }).catch((e) => setErr(String(e)));
  return (
    <>
      <section className="card form">
        <p className="dim">{tr("Janela transparente, sempre no topo e que ignora cliques, com as leituras escolhidas. Os itens podem ser trocados com o overlay aberto. No Wayland o compositor pode não manter a janela acima de jogos em tela cheia; em janela sem borda ou no X11 costuma funcionar.")}</p>
        <p className="dim">{tr("FPS (opcional): este app não injeta no jogo. Com o MangoHud instalado, coloque esta opção de inicialização no jogo (Steam: Propriedades > Opções de inicialização) e marque FPS (MangoHud) abaixo; sem log ativo o FPS mostra — (a pasta de log é fixa; ~ marca um valor de alguns segundos atrás).")}</p>
        <input readOnly className="mono" style={{ width: "100%" }} value={hint} onFocus={(e) => e.currentTarget.select()} />
        <div className="grid2">{OVERLAY_ITEMS.map(([k, label]) => (
          <label key={k} className="check"><input type="checkbox" checked={items.includes(k)} onChange={() => toggle(k)} /> {tr(label)}</label>
        ))}</div>
        <label>Canto
          <select value={corner} onChange={(e) => save(e.target.value, scale)}>
            <option value="TL">{tr("Superior esquerdo")}</option><option value="TR">{tr("Superior direito")}</option>
            <option value="BL">{tr("Inferior esquerdo")}</option><option value="BR">{tr("Inferior direito")}</option>
          </select>
        </label>
        <label>Tamanho: {scale.toFixed(1)}×<input type="range" min={0.8} max={2} step={0.1} value={scale} onChange={(e) => save(corner, +e.target.value)} /></label>
        <div><button className={open ? "btn danger" : "btn"} onClick={go}>{open ? tr("Fechar overlay") : tr("Abrir overlay")}</button>{err && <span className="err"> {err}</span>}</div>
      </section>
    </>
  );
}

function Bottleneck() {
  const m = useMetrics(1000);
  if (!m) return <Loading />;
  const vramPct = m.vram_used_mb && m.vram_total_mb ? (100 * m.vram_used_mb) / m.vram_total_mb : null;
  const ramPct = m.ram_used_gb && m.ram_total_gb ? (100 * m.ram_used_gb) / m.ram_total_gb : null;
  let verdict = tr("Sem carga relevante: abra um jogo e volte aqui."), lvl = "info";
  const gpu = m.gpu_util ?? 0, busy = m.cpu_busiest ?? 0;
  if ((m.gpu_temp ?? 0) >= 85 || (m.cpu_temp ?? 0) >= 90) { verdict = tr("Limite térmico: a temperatura está alta e pode estar reduzindo o clock."); lvl = "bad"; }
  else if (vramPct != null && vramPct >= 95) { verdict = tr("VRAM quase cheia: texturas menores ou resolução mais baixa evitam engasgos."); lvl = "bad"; }
  else if (ramPct != null && ramPct >= 92) { verdict = tr("RAM quase cheia: o sistema pode estar usando swap."); lvl = "warn"; }
  else if (gpu >= 95 && busy < 90) { verdict = tr("Limitado pela GPU: é o esperado; para mais FPS, reduza resolução ou qualidade gráfica."); lvl = "ok"; }
  else if (busy >= 90 && gpu < 85) { verdict = tr("Limitado pela CPU (um núcleo no limite): reduzir a GPU não ajuda; baixe simulação, multidões ou distância de visão."); lvl = "warn"; }
  else if (gpu >= 85 || busy >= 70) { verdict = tr("Carga alta e equilibrada entre GPU e CPU."); lvl = "ok"; }
  const row = (k: string, v: string) => <div className="kv" key={k}><span className="dim">{k}</span><span className="strong">{v}</span></div>;
  return (
    <>
      <section className="card"><div className={`verdict lv-${lvl}`}>{verdict}</div></section>
      <section className="card">
        {row(tr("GPU (uso)"), n0(m.gpu_util, "%"))}{row(tr("CPU total"), n0(m.cpu_load, "%", 0))}{row(tr("CPU núcleo mais ocupado"), n0(m.cpu_busiest, "%"))}
        {row("VRAM", vramPct == null ? "—" : `${vramPct.toFixed(0)} %`)}{row("RAM", ramPct == null ? "—" : `${ramPct.toFixed(0)} %`)}
        {row(tr("Temperatura GPU / CPU"), `${n0(m.gpu_temp, "°C")} / ${n0(m.cpu_temp, "°C")}`)}
        {m.gpu_throttle && row(tr("Limitador da GPU (PerfCap)"), m.gpu_throttle)}
      </section>
      <p className="dim">{tr("Leitura ao vivo: deixe o jogo rodando e olhe esta página (ou o overlay). Não analisa sessões gravadas.")}</p>
      <div className="cards"><Chart title={tr("Uso da GPU e da CPU")} lines={BOTTLENECK_LINES} unit="%" min={0} max={100} /></div>
    </>
  );
}

type Game = { store: string; id: string; name: string; install_dir: string; size_gb: number };
type FpsTable = { gpu: string; cpu: string; gpu_known: boolean; cpu_known: boolean;
  rows: { name: string; genre: string; avg: number; low1: number; limiter: string }[] };

function Games() {
  const [games, setGames] = useState<Game[]>([]);
  const [res, setRes] = useState("1920x1080");
  const [preset, setPreset] = useState("Alto");
  const [up, setUp] = useState("Desligado");
  const [tbl, setTbl] = useState<FpsTable | null>(null);
  useEffect(() => { invoke<Game[]>("games_scan").then((g) => setGames(g ?? [])); }, []);
  useEffect(() => { invoke<FpsTable>("fps_table", { res, preset, upscaling: up }).then(setTbl); }, [res, preset, up]);
  return (
    <>
      <h2 className="sub">{tr("Instalados")}</h2>
      <section className="card">
        {games.length === 0 && <p className="dim">{tr("Nenhum jogo encontrado (Steam, Lutris ou Heroic: Epic e GOG).")}</p>}
        {games.map((g) => (
          <div className="kv" key={g.store + g.id}>
            <span>{g.name} <span className="dim">{g.store} · {g.size_gb.toFixed(1)} GB</span></span>
            <button className="btn small" onClick={() => invoke("game_launch", { store: g.store, id: g.id })}>{tr("Jogar")}</button>
          </div>
        ))}
      </section>
      <h2 className="sub">{tr("FPS esperado (estimativa local)")}</h2>
      <section className="card form">
        <div className="row">
          <label>{tr("Resolução")}<select value={res} onChange={(e) => setRes(e.target.value)}><option>1920x1080</option><option>2560x1440</option><option>3840x2160</option></select></label>
          <label>{tr("Gráficos")}<select value={preset} onChange={(e) => setPreset(e.target.value)}><option value="Baixo">{tr("Baixo")}</option><option value="Médio">{tr("Médio")}</option><option value="Alto">{tr("Alto")}</option><option value="Ultra">Ultra</option></select></label>
          <label>Upscaling<select value={up} onChange={(e) => setUp(e.target.value)}><option value="Desligado">{tr("Desligado")}</option><option>Quality</option><option>Performance</option></select></label>
        </div>
        {tbl && <p className="dim">{tbl.gpu || "GPU?"}{!tbl.gpu_known && " " + tr("(fora da tabela: estimando com valor médio)")} · {tbl.cpu || "CPU?"}{!tbl.cpu_known && " " + tr("(fora da tabela)")}. {tr("Valores aproximados de benchmarks públicos, não medidos nesta máquina.")}</p>}
      </section>
      {tbl && (
        <table className="sensors">
          <thead><tr><th>{tr("Jogo")}</th><th>{tr("Gênero")}</th><th>{tr("FPS médio")}</th><th>{tr("1% low")}</th><th>{tr("Limite")}</th></tr></thead>
          <tbody>{tbl.rows.map((r) => (
            <tr key={r.name}><td>{r.name}</td><td className="dim">{tr(r.genre)}</td><td className="strong">{r.avg}</td><td className="dim">{r.low1}</td><td className="dim">{tr(r.limiter)}</td></tr>
          ))}</tbody>
        </table>
      )}
    </>
  );
}

type GpuBench = { running: boolean; progress: number; fps_now: number; frames: number; adapter: string; error: string;
  result: { adapter: string; width: number; height: number; seconds: number; frames: number; avg_fps: number; low1_fps: number; min_fps: number; score: number } | null };

function BenchGpu() {
  const [res, setRes] = useState("1920x1080");
  const [secs, setSecs] = useState(30);
  const [st, setSt] = useState<GpuBench | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => { const t = setInterval(() => invoke<GpuBench>("gpu_bench_status").then(setSt), 500); return () => clearInterval(t); }, []);
  const running = st?.running ?? false;
  const r = st?.result;
  const row = (k: string, v: string) => <div className="kv" key={k}><span className="dim">{k}</span><span className="strong">{v}</span></div>;
  return (
    <>
      <section className="card form">
        <p className="dim">{tr("Cena de jogo em Vulkan, fora da tela: terreno fractal, 40 mil árvores por instancing, sombras 4096² com PCF, água, neblina e pós HDR, com câmera em trajeto fixo. Nota = FPS médio × 10. Deixe a GPU livre durante o teste.")}</p>
        <div className="row">
          <label>{tr("Resolução")}<select value={res} disabled={running} onChange={(e) => setRes(e.target.value)}><option>1920x1080</option><option>2560x1440</option><option>3840x2160</option></select></label>
          <label>{tr("Duração")}<select value={secs} disabled={running} onChange={(e) => setSecs(+e.target.value)}><option value={30}>30 s</option><option value={60}>60 s</option></select></label>
        </div>
        <div>
          {running ? <button className="btn danger" onClick={() => invoke("gpu_bench_cancel")}>{tr("Cancelar")}</button>
                   : <button className="btn" onClick={() => invoke("gpu_bench_start", { res, seconds: secs }).then(() => setErr("")).catch((e) => setErr(String(e)))}>{tr("Iniciar")}</button>}
          {err && <span className="err"> {err}</span>}
        </div>
      </section>
      {st && (running || r || st.error) && (
        <section className="card">
          {running && <><div className="bar"><div style={{ width: `${st.progress * 100}%` }} /></div>{row("Agora", tf("{0} FPS · {1} quadros", st.fps_now.toFixed(0), st.frames))}</>}
          {st.error && <p className="err">{tr(st.error)}</p>}
          {r && <>
            <div className="score">{r.score.toFixed(0)} <small>pts · {r.width}×{r.height} · {r.adapter}</small></div>
            {row(tr("FPS médio"), r.avg_fps.toFixed(1))}{row(tr("1% low"), r.low1_fps.toFixed(1))}{row(tr("FPS mínimo"), r.min_fps.toFixed(1))}
            {row("Quadros", tf("{0} em {1} s", r.frames, r.seconds.toFixed(1)))}
          </>}
        </section>
      )}
      <RunHistory kind="bench-gpu" current={r ? { sig: JSON.stringify(r), value: r.score, detail: `${r.width}×${r.height}` } : null} />
    </>
  );
}

type StartupItem = { id: string; name: string; exec: string; scope: string; enabled: boolean };

function Startup() {
  const [items, setItems] = useState<StartupItem[]>([]);
  const [err, setErr] = useState("");
  const load = () => invoke<StartupItem[]>("startup_list").then((x) => setItems(x ?? []));
  useEffect(() => { load(); }, []);
  const set = (id: string, enabled: boolean) => invoke("startup_set", { id, enabled }).then(() => { setErr(""); load(); }).catch((e) => setErr(String(e)));
  return (
    <>
      <p className="dim">{tr("Programas que abrem com a sua sessão (autostart XDG). Desativar cria um ajuste só para o seu usuário e não mexe nos arquivos do sistema; reativar volta ao padrão.")}</p>
      {err && <p className="err">{err}</p>}
      <section className="card">
        {items.map((i) => (
          <div className="kv" key={i.id}>
            <span><b>{i.name}</b> <span className="dim">{tr(i.scope)} · {i.exec}</span></span>
            <label className="check"><input type="checkbox" checked={i.enabled} onChange={(e) => set(i.id, e.target.checked)} /> {i.enabled ? "Ativo" : "Desativado"}</label>
          </div>
        ))}
      </section>
    </>
  );
}

type SerialReport = { ports: { dev: string; chip: string; vid_pid: string; product: string; driver: string; access: boolean }[];
  usb: { vid_pid: string; name: string; chip: string; serial_ports: string[] }[]; in_dialout: boolean; brltty: boolean; hints: string[] };

function Serial() {
  const [r, setR] = useState<SerialReport | null>(null);
  const [msg, setMsg] = useState("");
  const load = () => invoke<SerialReport>("serial_report").then(setR);
  useEffect(() => { load(); const t = setInterval(load, 3000); return () => clearInterval(t); }, []);
  const fix = (action: string, what: string) => {
    if (!window.confirm(tf("{0}\n\nO sistema vai pedir a sua senha. Continuar?", what))) return;
    invoke<string>("serial_fix", { action }).then(() => { setMsg(tr("Feito. Para o grupo dialout valer, saia e entre na sessão de novo.")); load(); }).catch((e) => setMsg(String(e)));
  };
  if (!r) return <Loading />;
  return (
    <>
      {r.hints.map((h, i) => <section key={i} className="card"><p className="reason">{tr(h)}</p></section>)}
      <div className="row">
        {!r.in_dialout && <button className="btn" onClick={() => fix("dialout", tr("Adicionar seu usuário ao grupo dialout (sudo usermod -aG dialout)."))}>{tr("Adicionar ao grupo dialout")}</button>}
        {r.brltty && <button className="btn danger" onClick={() => fix("brltty", tr("Remover o pacote brltty."))}>{tr("Remover brltty")}</button>}
      </div>
      {msg && <p className="dim">{msg}</p>}
      <h2 className="sub">{tr("Portas seriais")}</h2>
      <section className="card">
        {r.ports.length === 0 && <p className="dim">{tr("Nenhuma porta /dev/ttyUSB* ou /dev/ttyACM* agora. Conecte a placa; a lista atualiza sozinha.")}</p>}
        {r.ports.map((p) => (
          <div className="kv" key={p.dev}>
            <span><b>{p.dev}</b> <span className="dim">{p.chip || tr("chip desconhecido")} · {p.product} · {p.vid_pid} · {tr("driver")} {p.driver || "—"}</span></span>
            <span className={`strong ${p.access ? "lv-ok" : "lv-bad"}`}>{p.access ? tr("acesso liberado") : tr("sem permissão")}</span>
          </div>
        ))}
      </section>
      <h2 className="sub">{tr("Dispositivos USB")}</h2>
      <section className="card">
        {r.usb.map((u, i) => (
          <div className="kv" key={i}>
            <span>{u.name || tr("(sem nome)")} <span className="dim">{u.chip}</span></span>
            <span className="dim">{u.vid_pid}{u.serial_ports.length ? ` · ${u.serial_ports.join(", ")}` : ""}</span>
          </div>
        ))}
      </section>
    </>
  );
}

type AppItem = { name: string; id: string; desc: string; installed: boolean; source: string };

function Apps() {
  const [apps, setApps] = useState<AppItem[] | null>(null);
  const [flatpak, setFlatpak] = useState(true);
  const [err, setErr] = useState("");
  const [st, setSt] = useState<{ running: boolean; app: string; output: string } | null>(null);
  const load = () => invoke<{ flatpak: boolean; apps: AppItem[] }>("apps_list").then((r) => { setApps(r.apps); setFlatpak(r.flatpak); }).catch((e) => setErr(String(e)));
  useEffect(() => {
    load();
    let was = false;
    const t = setInterval(async () => {
      const s = await invoke<{ running: boolean; app: string; output: string }>("app_install_status");
      setSt(s);
      if (was && !s.running) load();
      was = s.running;
    }, 1000);
    return () => clearInterval(t);
  }, []);
  const busy = st?.running ?? false;
  return (
    <>
      <p className="dim">{tr("Mostra o que já está instalado (Flatpak, Snap, pacote do sistema ou executável no PATH). O botão instala o que falta pelo Flathub, só para o seu usuário e sem senha.")}</p>
      {!flatpak && <p className="reason">{tr("Flatpak não encontrado: para instalar por aqui, instale o pacote flatpak da sua distribuição e reabra o app.")}</p>}
      {err && <p className="err">{err}</p>}
      <section className="card">
        {apps?.map((a) => (
          <div className="kv" key={a.id}>
            <span><b>{a.name}</b> <span className="dim">{tr(a.desc)}</span></span>
            {a.installed ? <span className="lv-ok strong">{tr("Instalado")} · {tr(a.source)}</span>
              : <button className="btn small" disabled={busy || !flatpak} onClick={() => window.confirm(tf("Instalar {0} pelo Flatpak? O download pode ser grande e leva alguns minutos.", a.name)) && invoke("app_install", { id: a.id }).catch((e) => setErr(String(e)))}>{tr("Instalar")}</button>}
          </div>
        ))}
      </section>
      {st && (st.running || st.output) && (
        <section className="card"><div className="dim">{st.running ? tf("Instalando {0}…", st.app) : st.app}</div><pre className="log">{st.output}</pre></section>
      )}
    </>
  );
}

type RtStatus = { running: boolean; phase: string; progress: number; fps_now: number; adapter: string; error: string;
  result: { adapter: string; width: number; height: number; preset: string; pass_fps: number[]; median_fps: number; score: number } | null };

function RayTracing() {
  const [res, setRes] = useState("1280x720");
  const [preset, setPreset] = useState("Alta");
  const [st, setSt] = useState<RtStatus | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => { const t = setInterval(() => invoke<RtStatus>("rt_status").then(setSt), 500); return () => clearInterval(t); }, []);
  const running = st?.running ?? false;
  const r = st?.result;
  const row = (k: string, v: string) => <div className="kv" key={k}><span className="dim">{k}</span><span className="strong">{v}</span></div>;
  return (
    <>
      <section className="card form">
        <p className="dim">{tr("Path tracing com ray queries do Vulkan (núcleos de RT de RTX 20+, Radeon RX 6000+, Intel Arc): 4 mil esferas fosco, metal, espelho e luz, sombra suave, reflexos e iluminação indireta, com a estrutura de aceleração refeita a cada quadro. Aquecimento, 3 passadas de 10 s e nota pela mediana (FPS × 10). Recurso experimental do wgpu: pode falhar em drivers antigos.")}</p>
        <div className="row">
          <label>{tr("Resolução")}<select value={res} disabled={running} onChange={(e) => setRes(e.target.value)}><option>1280x720</option><option>1920x1080</option></select></label>
          <label>{tr("Preset")}<select value={preset} disabled={running} onChange={(e) => setPreset(e.target.value)}>
            <option value="Normal">{tr("Normal (2 amostras, 2 rebatidas)")}</option><option value="Alta">{tr("Alta (4 amostras, 3 rebatidas)")}</option><option value="Extrema">{tr("Extrema (8 amostras, 4 rebatidas)")}</option></select></label>
        </div>
        <div>
          {running ? <button className="btn danger" onClick={() => invoke("rt_cancel")}>{tr("Cancelar")}</button>
                   : <button className="btn" onClick={() => invoke("rt_start", { res, preset }).then(() => setErr("")).catch((e) => setErr(String(e)))}>{tr("Iniciar")}</button>}
          {err && <span className="err"> {err}</span>}
        </div>
      </section>
      {st && (running || r || st.error) && (
        <section className="card">
          {running && <><div className="bar"><div style={{ width: `${st.progress * 100}%` }} /></div>{row(tr(st.phase), `${st.fps_now.toFixed(1)} FPS`)}</>}
          {st.error && <p className="err">{tr(st.error)}</p>}
          {r && <>
            <div className="score">{r.score.toFixed(0)} <small>pts · {r.width}×{r.height} · {r.preset} · {r.adapter}</small></div>
            {row("Mediana", `${r.median_fps.toFixed(1)} FPS`)}{row("Passadas", r.pass_fps.map((f) => f.toFixed(1)).join(" · "))}
          </>}
        </section>
      )}
      <RunHistory kind="rt" current={r ? { sig: JSON.stringify(r), value: r.score, detail: `${r.width}×${r.height} · ${tr(r.preset)}` } : null} />
    </>
  );
}

type Channel = { id: string; kind: string; label: string; chip: string; hw: string; pwm: number; rpm: number | null; percent: number | null;
  mode: string; controllable: boolean; min: number; max: number; fan: number | null; tested: boolean; responsive: boolean };

/** Ventoinha detectada que não dá para controlar: só leitura, ou um controle que o teste mostrou não mover nenhuma ventoinha. */
const fanOff = (c: Channel) => c.kind === "hwmon" && (!c.controllable || (c.tested && c.fan == null));

function FanRow({ c, onDone }: { c: Channel; onDone: (m: string) => void }) {
  const [v, setV] = useState(Math.max(c.min, Math.round(c.percent ?? 50)));
  const [busy, setBusy] = useState(false);
  const go = (percent: number | null) => {
    const what = percent == null ? tr("devolver esta ventoinha ao controle automático") : tf("fixar esta ventoinha em {0} %", percent);
    if (!window.confirm(tf("Vou {0}.\n\nUma ventoinha parada ou lenta demais pode superaquecer o hardware. O sistema vai pedir a sua senha. Continuar?", what))) return;
    setBusy(true);
    invoke<string>("fan_apply", { kind: c.kind, hw: c.hw, pwm: c.pwm, chip: c.chip, percent }).then((m) => onDone(`${c.label}: ${tr(m)}`)).catch((e) => onDone(`${c.label}: ${tr(String(e))}`)).finally(() => setBusy(false));
  };
  const off = fanOff(c);
  return (
    <div className={off ? "fan off" : "fan"}>
      <div className="fan-head"><b>{tr(c.label)}</b>{off && <span className="badge off" title={tr("Detectada, mas sem controle de velocidade")}>{tr("Desativada")}</span>}</div>
      <div className="dim">{c.chip} · {tr(c.mode)}</div>
      <div className="fan-stats">
        <span><span className="big">{c.rpm != null ? c.rpm : "—"}</span> RPM</span>
        <span><span className="big">{c.percent != null ? c.percent.toFixed(0) : "—"}</span> %</span>
      </div>
      <div className="bar"><div style={{ width: `${c.percent ?? 0}%` }} /></div>
      {c.kind === "hwmon" && c.tested && c.fan == null && <p className="reason">{tr("Sem efeito: o teste não achou nenhuma ventoinha que reaja a este controle (conector vazio ou ventoinha sem controle de velocidade). Mudar a velocidade aqui não vai ter efeito visível.")}</p>}
      {c.kind === "hwmon" && c.tested && c.fan != null && <p className="dim">{tf("O teste confirmou: este controle move a entrada de ventoinha {0}.", c.fan)}</p>}
      {c.kind === "hwmon" && !c.tested && c.rpm === 0 && <p className="reason">{tr("0 RPM: este canal não recebe sinal de nenhuma ventoinha (conector vazio, ou ventoinha fora do alcance do chip, por exemplo num hub). Mudar a velocidade aqui não vai ter efeito visível.")}</p>}
      {c.controllable && !off ? (
        <div className="row fan-ctl">
          <input type="range" min={c.min} max={c.max} value={v} onChange={(e) => setV(+e.target.value)} aria-label={tf("Velocidade de {0}", c.label)} />
          <span className="strong">{v} %</span>
          <button className="btn small" disabled={busy} onClick={() => go(v)}>{tr("Aplicar")}</button>
          <button className="btn small ghost" disabled={busy} onClick={() => go(null)}>{tr("Automático")}</button>
        </div>
      ) : !c.controllable && <p className="dim">{tr("Só leitura: o kernel não expõe controle de velocidade para este canal.")}</p>}
    </div>
  );
}

function Fans() {
  const [r, setR] = useState<{ channels: Channel[]; hints: string[]; modules: string[] } | null>(null);
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState("");
  const [driver, setDriver] = useState("");
  const [manual, setManual] = useState(0);
  const load = () => invoke<{ channels: Channel[]; hints: string[]; modules: string[] }>("fans_report").then(setR);
  useEffect(() => { const f = () => { load(); invoke<unknown[]>("fan_overrides").then((o) => setManual(o.length)); }; f(); const t = setInterval(f, 2000); return () => clearInterval(t); }, []);
  if (!r) return <Loading />;
  const restore = () => { if (window.confirm(tr("Devolver ao controle automático todas as ventoinhas que o CoreForge fixou? O sistema vai pedir a sua senha."))) invoke<string>("fan_restore_all").then((m) => { setMsg(m); load(); }).catch((e) => setMsg(String(e))); };
  const scan = () => {
    if (!window.confirm(tr("Vou procurar o driver das ventoinhas da sua placa-mãe, tentando carregar os candidatos do seu fabricante (modprobe) e descarregando os que não ajudarem. O sistema vai pedir a sua senha. Continuar?"))) return;
    setBusy(tr("Procurando controladores…"));
    invoke<string>("fan_scan_modules").then((out) => {
      const lines = out.split("\n"); const pick = (k: string) => lines.filter((l) => l.startsWith(k + ":")).map((l) => l.split(":").slice(1).join(":"));
      const kept = pick("kept"), already = pick("already"), none = pick("none"), err = pick("error");
      setDriver(kept[0] ?? already[0] ?? "");
      setMsg(kept.length ? tf("Achei e carreguei o driver {0}. As ventoinhas devem aparecer abaixo.", kept[0]) : already.length ? tf("O driver {0} já estava carregado.", already[0]) : tr("Nenhum driver candidato revelou ventoinhas novas.")
        + (none.length ? " " + tf("Tentei sem sucesso: {0}.", none.join(", ")) : "") + (err.length ? " " + tf("Erros: {0}.", err.join("; ")) : "") + " " + tr("Se a sua placa controla só pela BIOS, não há o que fazer por aqui."));
      load();
    }).catch((e) => setMsg(String(e))).finally(() => setBusy(""));
  };
  const chips = [...new Map(r.channels.filter((c) => c.kind === "hwmon" && c.controllable).map((c) => [c.hw, c])).values()];
  const identify = async () => {
    if (!window.confirm(tr("Vou identificar qual controle move qual ventoinha. Cada controle é levado a 100 % e depois a 35 % por alguns segundos (cerca de 8 s por controle) e volta ao que estava. Deixe o PC ocioso. O sistema vai pedir a sua senha. Continuar?"))) return;
    for (const c of chips) {
      setBusy(tf("Testando {0}… (pode levar um minuto)", c.chip));
      try { setMsg(`${c.chip}: ${await invoke<string>("fan_map_test", { hw: c.hw, chip: c.chip })}`); } catch (e) { setMsg(`${c.chip}: ${e}`); break; }
    }
    setBusy(""); load();
  };
  return (
    <>
      <p className="dim">{tr("Mudanças de velocidade pedem a senha do sistema e valem até o PC reiniciar ou você tocar em Automático. O mínimo é 25 %.")}</p>
      {r.hints.map((h, i) => <section key={i} className="card"><p className="reason">{tr(h)}</p></section>)}
      {manual > 0 && <section className="card"><p className="reason">{tf("O CoreForge fixou a velocidade de {0} ventoinha(s). Elas ficam assim até o PC reiniciar. Se for deixar o app fechado, é mais seguro devolvê-las ao automático.", manual)}</p><button className="btn small" onClick={restore}>{tr("Devolver tudo ao automático")}</button></section>}
      <div className="row">
        <button className="btn small" disabled={!!busy} onClick={scan}>{tr("Procurar controladores")}</button>
        {chips.length > 0 && <button className="btn small" disabled={!!busy} onClick={identify}>{tr("Identificar controles")}</button>}
      </div>
      {driver && <div><button className="btn small ghost" onClick={() => { if (window.confirm(tf("Fazer o driver {0} carregar em todo boot? Cria um arquivo em /etc/modules-load.d e pede a sua senha.", driver))) invoke<string>("fan_persist_module", { name: driver }).then(setMsg).catch((e) => setMsg(String(e))); }}>{tf("Manter {0} ativo após reiniciar", driver)}</button></div>}
      {busy && <p className="dim">{busy}</p>}
      {msg && <p className="dim">{msg}</p>}
      {r.channels.length === 0 && <section className="card"><p className="dim">{tr("Nenhuma ventoinha encontrada.")}</p></section>}
      <div className="cards">
        {[...r.channels].sort((a, b) => Number(fanOff(a)) - Number(fanOff(b))).map((c) => (
          <section className={fanOff(c) ? "card fan-card off" : "card fan-card"} key={c.id}><FanRow c={c} onDone={(m) => { setMsg(m); load(); }} /></section>
        ))}
      </div>
    </>
  );
}

type LedDevice = { index: number; name: string; kind: string; modes: string[]; active_mode: string; zones: string[]; zone_leds: number[]; led_names: string[][] };
type Discovery = { openrgb: string; hid: { vid_pid: string; vendor: string; name: string; accessible: boolean }[];
  i2c: { dev: string; adapter: string; kind: string; accessible: boolean }[]; i2c_dev_loaded: boolean; hints: string[];
  gpus: { name: string; slot: string; ids: string; subsystem: string; board_maker: string; i2c_buses: string[]; i2c_accessible: boolean }[] };

/** Cor por LED: escolhe os LEDs clicando e dá uma cor a cada um (só no modo Direct). */
function PerLed({ d, zone, count, base, onDone }: { d: LedDevice; zone: number; count: number; base: string; onDone: (m: string) => void }) {
  const [cols, setCols] = useState<string[]>([]);
  const [sel, setSel] = useState<number[]>([]);
  const [pick, setPick] = useState("#ff3b30");
  const [busy, setBusy] = useState(false);
  // ao mudar de zona ou de quantidade, todos os LEDs partem da cor base (nada fica preto sem querer)
  useEffect(() => { setCols(Array(count).fill(base)); setSel([]); }, [zone, count]);
  const names = d.led_names[zone] ?? [];
  const toggle = (i: number) => setSel((s) => (s.includes(i) ? s.filter((x) => x !== i) : [...s, i]));
  const paint = (c: string) => { setPick(c); setCols((cs) => cs.map((x, i) => (sel.includes(i) ? c : x))); };
  const send = () => {
    setBusy(true);
    invoke<string>("leds_set", { device: d.index, deviceName: d.name, zoneName: d.zones[zone], zone, mode: "Direct", colors: cols.map((c) => c.replace("#", "")) })
      .then(() => onDone(tf("{0}: aplicado", d.name))).catch((e) => onDone(`${d.name}: ${e}`)).finally(() => setBusy(false));
  };
  return (
    <div className="perled">
      <div className="leds-strip">
        {cols.map((c, i) => (
          <button key={i} className={sel.includes(i) ? "led sel" : "led"} style={{ background: c }} onClick={() => toggle(i)}
            title={names[i] ? `${names[i]}` : `LED ${i + 1}`} aria-label={`LED ${i + 1}`} aria-pressed={sel.includes(i)} />
        ))}
      </div>
      <div className="row fan-ctl">
        <input type="color" value={pick} onChange={(e) => paint(e.target.value)} aria-label={tr("Cor dos LEDs selecionados")} disabled={sel.length === 0} />
        <span className="dim">{tf("{0} de {1} LEDs selecionados", sel.length, count)}</span>
        <button className="btn small ghost" onClick={() => setSel(cols.map((_, i) => i))}>{tr("Todos")}</button>
        <button className="btn small ghost" onClick={() => setSel([])}>{tr("Nenhum")}</button>
        <button className="btn small" disabled={busy} onClick={send}>{tr("Aplicar LED por LED")}</button>
      </div>
    </div>
  );
}

function LedRow({ d, onDone }: { d: LedDevice; onDone: (m: string) => void }) {
  const [color, setColor] = useState("#b4ff1f");
  const [mode, setMode] = useState(d.active_mode || d.modes[0] || "");
  const [bright, setBright] = useState(100);
  const [zone, setZone] = useState(-1);
  const [leds, setLeds] = useState(48);
  const [each, setEach] = useState(false);
  const addressable = zone >= 0 && /addressable/i.test(d.zones[zone] ?? "");
  const [busy, setBusy] = useState(false);
  const apply = (over: { mode?: string; color?: string } = {}) => {
    setBusy(true);
    invoke<string>("leds_set", { device: d.index, deviceName: d.name, zoneName: zone >= 0 ? d.zones[zone] : null, zone: zone >= 0 ? zone : null, size: addressable ? leds : null, mode: over.mode ?? mode, color: (over.color ?? color).replace("#", ""), brightness: bright, speed: null })
      .then(() => onDone(tf("{0}: aplicado", d.name))).catch((e) => onDone(`${d.name}: ${e}`)).finally(() => setBusy(false));
  };
  // Sem modo "Off" (ex.: memória HyperX), desligar = preto fixo no modo Direct.
  const off = d.modes.find((m) => m.toLowerCase() === "off");
  const turnOff = () => (off ? apply({ mode: off }) : apply({ mode: d.modes.find((m) => m === "Direct") ?? d.modes[0], color: "#000000" }));
  return (
    <div className="fan">
      <div className="kv"><span><b>{d.name}</b> <span className="dim">{d.kind}</span></span><span className="dim">modo atual: {d.active_mode || "—"}</span></div>
      <div className="row fan-ctl">
        <input type="color" value={color} onChange={(e) => setColor(e.target.value)} aria-label={tr("Cor")} />
        <select value={zone} onChange={(e) => setZone(+e.target.value)} aria-label={tr("Zona")}><option value={-1}>{tr("Todas as zonas")}</option>{d.zones.map((z, i) => <option key={z} value={i}>{z}</option>)}</select>
        {addressable && <label className="check">{tr("LEDs")}<input type="number" min={1} max={1024} value={leds} onChange={(e) => setLeds(+e.target.value)} style={{ minWidth: 80, width: 80 }} aria-label={tr("Quantidade de LEDs")} /></label>}
        <select value={mode} onChange={(e) => setMode(e.target.value)} aria-label={tr("Modo")}>{d.modes.map((m) => <option key={m}>{m}</option>)}</select>
        <label className="check">{tf("Brilho {0} %", bright)}<input type="range" min={0} max={100} value={bright} onChange={(e) => setBright(+e.target.value)} /></label>
        {addressable && (d.zone_leds[zone] ?? 0) === 0 && <span className="reason">{tr("Zona sem LEDs configurados: informe a quantidade ao lado e aplique.")}</span>}
        <button className="btn small" disabled={busy} onClick={() => apply()}>{tr("Aplicar")}</button>
        <button className="btn small danger" disabled={busy} onClick={turnOff}>{tr("Desligar")}</button>
        {zone >= 0 && d.modes.includes("Direct") && <button className="btn small ghost" onClick={() => setEach(!each)} aria-expanded={each}>{each ? tr("Ocultar LED por LED") : tr("LED por LED")}</button>}
      </div>
      {each && zone >= 0 && d.modes.includes("Direct") && (() => {
        const count = addressable ? Math.max(d.zone_leds[zone] ?? 0, leds) : (d.zone_leds[zone] ?? 0);
        return count > 0 ? <PerLed d={d} zone={zone} count={count} base={color} onDone={onDone} /> : <p className="reason">{tr("Zona sem LEDs configurados: informe a quantidade ao lado e aplique.")}</p>;
      })()}
    </div>
  );
}

// Último resultado de cada leitura: ao voltar à aba, a tela aparece na hora e é atualizada em segundo plano.
let discoCache: Discovery | null = null;
let ledsCache: { backend: string; installed: boolean; devices: LedDevice[]; hints: string[] } | null = null;

function RgbDiscovery({ onChanged, devices }: { onChanged: () => void; devices: LedDevice[] }) {
  const [d, setD] = useState<Discovery | null>(discoCache);
  const [withI2c, setWithI2c] = useState(false);
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState("");
  const load = () => invoke<Discovery>("leds_discover").then((x) => { discoCache = x; setD(x); });
  useEffect(() => { load(); }, []);
  if (!d) return <Loading />;
  const hidTodo = d.hid.filter((h) => !h.accessible), i2cTodo = d.i2c.filter((b) => !b.accessible);
  const need = hidTodo.length > 0 || !d.i2c_dev_loaded || i2cTodo.length > 0;
  const grant = () => {
    const kinds = [...new Set(i2cTodo.map((b) => b.kind))];
    const what = [hidTodo.length ? tf("acesso aos dispositivos USB: {0}", hidTodo.map((h) => `${h.vendor} ${h.vid_pid}`).join(", ")) : "", !d.i2c_dev_loaded ? tr("carregar o módulo i2c-dev") : "",
      withI2c && kinds.length ? tf("acesso aos barramentos I²C ({0})", kinds.join(", ")) : ""].filter(Boolean).join("; ");
    if (!what || !window.confirm(tf("Vou liberar: {0}.\n\nIsso cria regras udev só para esses dispositivos. O sistema vai pedir a sua senha. Continuar?", what))) return;
    setBusy(true);
    invoke<string>("leds_setup", { hid: hidTodo.map((h) => h.vid_pid), i2c: withI2c ? kinds : [], loadI2c: !d.i2c_dev_loaded })
      .then(() => { setMsg(tr("Acesso liberado. Se um dispositivo não aparecer, reconecte-o.")); load(); onChanged(); }).catch((e) => setMsg(String(e))).finally(() => setBusy(false));
  };
  // Veredito por placa de vídeo: o OpenRGB só a lista se reconhece o modelo (pelo ID de subsistema) e se há LED endereçável.
  const gpuVerdict = (g: Discovery["gpus"][number]) => {
    if (devices.some((x) => x.kind.toLowerCase().includes("gpu"))) return { cls: "lv-ok", text: tr("LED controlável: aparece na lista abaixo") };
    if (g.i2c_buses.length === 0) return { cls: "lv-warn", text: tr("Sem barramento I²C visível (driver da placa ou módulo i2c-dev ausente)") };
    if (!g.i2c_accessible) return { cls: "lv-warn", text: tr("Barramento I²C sem permissão: libere o acesso abaixo (opção de memória e placa de vídeo)") };
    return { cls: "dim", text: tr("O OpenRGB não lista LED para este modelo: ou a placa não tem LED controlável, ou ainda não é suportada. Copie o relatório para pedir suporte.") };
  };
  const makeReport = () => invoke<string>("leds_report").then((t) => { setReport(t); navigator.clipboard?.writeText(t).catch(() => {}); });
  const ok = (b: boolean) => <span className={b ? "lv-ok strong" : "lv-warn strong"}>{b ? tr("acesso liberado") : tr("sem permissão")}</span>;
  return (
    <>
      <h2 className="sub">{tr("Hardware encontrado")}</h2>
      <section className="card">
        <div className="kv"><span>OpenRGB</span><span className={d.openrgb ? "lv-ok strong" : "lv-warn strong"}>{d.openrgb ? tf("instalado ({0})", d.openrgb) : tr("não instalado")}</span></div>
        {d.gpus.map((g) => { const v = gpuVerdict(g); return (
          <div className="kv" key={g.slot}><span>{tr("Placa de vídeo")} <span className="dim">{g.name} · {g.board_maker} (subsistema {g.subsystem})</span></span><span className={`strong ${v.cls}`} style={{ maxWidth: 360 }}>{v.text}</span></div>
        ); })}
        {d.hid.map((h) => <div className="kv" key={h.vid_pid}><span>{h.vendor} <span className="dim">{h.name} · {h.vid_pid}</span></span>{ok(h.accessible)}</div>)}
        {d.i2c.length > 0 && <div className="kv"><span>{tr("Memória e placa de vídeo")} <span className="dim">(barramentos I²C: {d.i2c.map((b) => b.dev.replace("/dev/", "")).join(", ")})</span></span>{ok(d.i2c.every((b) => b.accessible))}</div>}
        {d.hid.length === 0 && d.i2c.length === 0 && <p className="dim">{tr("Nenhum dispositivo RGB conhecido por fabricante foi encontrado. Dispositivos Bluetooth e alguns controladores próprios não aparecem aqui.")}</p>}
        {d.hints.map((h, i) => <p key={i} className="reason">{tr(h)}</p>)}
        {need && (
          <div className="form">
            {i2cTodo.length > 0 && <label className="check"><input type="checkbox" checked={withI2c} onChange={(e) => setWithI2c(e.target.checked)} /> {tr("Incluir memória e placa de vídeo (I²C). O SMBus também fala com sensores e a regulagem de energia da placa: uma escrita errada pode travar o PC. O uso normal do OpenRGB é seguro, mas a permissão é sua.")}</label>}
            <div><button className="btn small" disabled={busy} onClick={grant}>{tr("Liberar acesso")}</button></div>
          </div>
        )}
        {msg && <p className="dim">{msg}</p>}
        <div><button className="btn small ghost" onClick={makeReport}>{tr("Copiar relatório de hardware")}</button></div>
        {report && <pre className="log">{report}</pre>}
      </section>
    </>
  );
}

function Leds() {
  const [r, setR] = useState(ledsCache);
  const [msg, setMsg] = useState("");
  const [loading, setLoading] = useState(true);
  const load = () => { setLoading(true); invoke<NonNullable<typeof ledsCache>>("leds_status").then((x) => { ledsCache = x; setR(x); }).finally(() => setLoading(false)); };
  useEffect(() => { load(); }, []);
  return (
    <>
      <p className="dim">{tr("Usa o OpenRGB, que fala com a placa-mãe, as ventoinhas, a memória e muitos periféricos. A leitura leva alguns segundos.")}</p>
      {loading && <p className="dim">{tr("Procurando dispositivos…")}</p>}
      {r?.hints.map((h, i) => <section key={i} className="card"><p className="reason">{tr(h)}</p></section>)}
      {r && !r.installed && <div><button className="btn" onClick={() => invoke("app_install", { id: "org.openrgb.OpenRGB" }).then(() => setMsg(tr("Instalando o OpenRGB: acompanhe em Instalar apps."))).catch((e) => setMsg(String(e)))}>{tr("Instalar o OpenRGB (Flatpak)")}</button></div>}
      {msg && <p className="dim">{msg}</p>}
      <RgbDiscovery onChanged={load} devices={r?.devices ?? []} />
      {r && r.devices.length > 0 && <div className="cards cards-2">{r.devices.map((d) => <section className="card" key={d.index}><LedRow d={d} onDone={setMsg} /></section>)}</div>}
      {r?.installed && <button className="btn small ghost" onClick={load}>{tr("Procurar de novo")}</button>}
    </>
  );
}

const fmt = (v: number, unit: string) => `${v.toFixed(unit === "MHz" || unit === "RPM" ? 0 : 1)} ${unit}`;

function Sensors() {
  const [rows, setRows] = useState<Sensor[]>([]);
  const [ranges, setRanges] = useState<Record<string, Range>>({});
  const [kind, setKind] = useState("");
  const [q, setQ] = useState("");
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      const r = await invoke<Sensor[]>("sensors");
      if (!alive) return;
      setRows(r);
      setRanges((prev) => {
        const next = { ...prev };
        for (const s of r) {
          const p = next[s.id];
          next[s.id] = p ? { min: Math.min(p.min, s.value), max: Math.max(p.max, s.value) } : { min: s.value, max: s.value };
        }
        return next;
      });
    };
    tick();
    const t = setInterval(tick, 1000);
    return () => { alive = false; clearInterval(t); };
  }, []);
  const kinds = [...new Set(rows.map((r) => r.category))];
  const shown = rows.filter((r) => (!kind || r.category === kind) && (!q || `${r.hardware} ${r.name}`.toLowerCase().includes(q.toLowerCase())));
  return (
    <>
    <div className="cards cards-3">
      <Chart title={tr("Temperaturas")} lines={TEMP_LINES} unit="°C" />
      <Chart title={tr("Carga")} lines={LOAD_LINES} unit="%" min={0} max={100} />
      <Chart title={tr("Consumo da GPU")} lines={POWER_LINES} unit="W" min={0} />
    </div>
    <div className="row filters">
      <input type="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder={tr("Buscar sensor")} aria-label={tr("Buscar sensor")} />
      <select value={kind} onChange={(e) => setKind(e.target.value)} aria-label={tr("Tipo")}>
        <option value="">{tr("Todos os tipos")}</option>{kinds.map((k) => <option key={k} value={k}>{tr(k)}</option>)}
      </select>
      <span className="dim">{shown.length} / {rows.length}</span>
    </div>
    <table className="sensors">
      <thead><tr><th>{tr("Hardware")}</th><th>{tr("Tipo")}</th><th>{tr("Sensor")}</th><th>{tr("Valor")}</th><th>{tr("Mín")}</th><th>{tr("Máx")}</th></tr></thead>
      <tbody>
        {shown.map((s) => (
          <tr key={s.id}>
            <td className="dim">{s.hardware}</td><td className="dim">{tr(s.category)}</td><td>{s.name}</td>
            <td className="strong">{fmt(s.value, s.unit)}</td>
            <td className="dim">{fmt(ranges[s.id]?.min ?? s.value, s.unit)}</td>
            <td className="dim">{fmt(ranges[s.id]?.max ?? s.value, s.unit)}</td>
          </tr>
        ))}
      </tbody>
    </table>
    </>
  );
}

function Settings() {
  const [limit, setLimit] = useState(tempLimit());
  const [keep, setKeep] = useState(localStorage.getItem("keepTray") === "1");
  return (
    <section className="card">
      <h2>{tr("Preferências")}</h2>
      <div className="row filters">
        <label className="dim">{tr("Alerta de temperatura (°C)")}{" "}
          <input type="number" min={0} max={120} value={limit} style={{ width: 70 }}
            onChange={(e) => { const v = Math.max(0, Math.min(120, Number(e.target.value) || 0)); setLimit(v); localStorage.setItem("tempLimit", String(v)); }} />
        </label>
        <label className="dim">
          <input type="checkbox" checked={keep} onChange={(e) => { setKeep(e.target.checked); localStorage.setItem("keepTray", e.target.checked ? "1" : "0"); invoke("tray_close_set", { on: e.target.checked }); }} /> {tr("Manter na bandeja ao fechar")}
        </label>
        <span className="dim">{tr("Idioma")}</span>
        <div className="langs" role="group" aria-label="Language">
          {(["en", "pt"] as const).map((l) => <button key={l} className={l === lang ? "active" : ""} aria-pressed={l === lang} onClick={() => l !== lang && setLang(l)}>{l.toUpperCase()}</button>)}
        </div>
      </div>
    <p className="dim hint">{tr("Avisa quando a CPU ou a GPU passa desse valor. 0 desliga.")} {tr("Fechar a janela deixa o app na bandeja do sistema, e os alertas continuam. Precisa de bandeja no desktop.")}</p>
    </section>
  );
}

/** Boas-vindas da primeira execução: aparece uma vez (guardado em localStorage). */
function Welcome({ onSetup }: { onSetup: () => void }) {
  const [open, setOpen] = useState(!localStorage.getItem("welcomed"));
  const box = useRef<HTMLDivElement>(null);
  const close = (setup: boolean) => { localStorage.setItem("welcomed", "1"); setOpen(false); if (setup) onSetup(); };
  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    box.current?.querySelector<HTMLElement>("button")?.focus();
    return () => prev?.focus?.();
  }, [open]);
  if (!open) return null;
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") return close(false);
    if (e.key !== "Tab") return;
    const f = box.current!.querySelectorAll<HTMLElement>("button");
    const first = f[0], last = f[f.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  };
  return (
    <div className="welcome" ref={box} onKeyDown={onKey} role="dialog" aria-modal="true" aria-label={tr("Bem-vindo ao CoreForge")}>
      <section className="card">
        <h2>{tr("Bem-vindo ao CoreForge")}</h2>
        <ul>
          <li>{tr("Monitora CPU, GPU, discos e sensores ao vivo.")}</li>
          <li>{tr("Testa e controla o hardware: stress, benchmarks, ventoinhas e LEDs.")}</li>
          <li>{tr("Escritas no hardware pedem a senha do sistema.")}</li>
        </ul>
        <p className="dim">{tr("Comece pela página Preparação: ela mostra o que falta neste PC.")}</p>
        <div className="row">
          <button className="btn" onClick={() => close(true)}>{tr("Abrir Preparação")}</button>
          <button className="btn ghost" onClick={() => close(false)}>{tr("Fechar")}</button>
        </div>
      </section>
    </div>
  );
}

// Navegação: poucas seções, cada uma com abas internas. O texto de apoio muda conforme a aba.
type Ctx = { setIssues: (n: number) => void; goLeds: () => void };
type Tab = { id: string; label: string; blurb: string; view: (c: Ctx) => ReactNode };
/** Agrupamento e ícone do menu lateral (24×24, traço). */
const NAV_GROUP: Record<string, string> = { sistema: "Monitorar", disco: "Monitorar", stress: "Testar", bench: "Testar", jogos: "Testar", controle: "Controlar", ferramentas: "Controlar", config: "Controlar" };
const NAV_ICON: Record<string, string> = {
  setup: "M5 12l4 4 10-10", sistema: "M4 5h16v11H4zM9 20h6M12 16v4", disco: "M4 7c0-2 16-2 16 0v10c0 2-16 2-16 0zM4 12c0 2 16 2 16 0",
  stress: "M13 3L5 14h6l-1 7 8-11h-6z", bench: "M5 20V10M12 20V4M19 20v-7", jogos: "M6 9h12a3 3 0 013 3v3a2 2 0 01-3.5 1.3L15 14H9l-2.5 2.300A2 2 0 013 15v-3a3 3 0 013-3zM8 11v2M7 12h2",
  controle: "M12 12m-2 0a2 2 0 104 0 2 2 0 10-4 0M12 3v4M12 17v4M3 12h4M17 12h4M5.600 5.600l2.800 2.800M15.600 15.600l2.800 2.800", ferramentas: "M14 6a4 4 0 005 5l-9 9a2 2 0 01-3-3l9-9a4 4 0 00-2-2z", config: "M4 7h10M18 7h2M4 17h2M10 17h10M16 5v4M8 15v4",
};
type Section = { id: string; title: string; tabs: Tab[] };
const SECTIONS: Section[] = [
  { id: "setup", title: "Preparação", tabs: [{ id: "setup", label: "Preparação", blurb: tr("O que este PC precisa para o CoreForge funcionar por completo, verificado sozinho."), view: (c) => <Setup onIssues={c.setIssues} go={(name) => name === "LEDs" && c.goLeds()} /> }] },
  { id: "sistema", title: "Sistema", tabs: [
    { id: "info", label: "Informações", blurb: tr("Sistema, placa-mãe, processador, memória, vídeo e discos desta máquina."), view: () => <Info /> },
    { id: "gpu", label: "Placa de vídeo", blurb: tr("Ficha da placa de vídeo (NVIDIA, AMD e Intel) e leituras ao vivo."), view: () => <Gpu /> },
    { id: "cpu", label: "CPU", blurb: tr("Detalhes do processador: cache, frequência por núcleo, governador e instruções."), view: () => <CpuDetails /> },
    { id: "limites", label: "Limites", blurb: tr("Verifica se a CPU ou a GPU estão sendo limitadas por calor ou potência."), view: () => <Throttle /> },
    { id: "energia", label: "Energia", blurb: tr("Bateria, fonte de energia e perfil de desempenho (notebooks)."), view: () => <Power /> },
    { id: "historico", label: "Histórico", blurb: tr("Últimos 10 minutos de temperatura, carga, consumo e ventoinha, com mínimo, média e máximo."), view: () => <History /> },
    { id: "sensores", label: "Sensores", blurb: tr("Temperaturas, clocks, cargas e tensões, atualizados a cada segundo."), view: () => <Sensors /> }] },
  { id: "disco", title: "Disco e rede", tabs: [
    { id: "disco", label: "Disco", blurb: tr("Mede velocidade e IOPS do disco e mostra a saúde dele."), view: () => <Disk /> },
    { id: "rede", label: "Rede", blurb: tr("Latência, perda de pacotes, DNS e velocidade de internet."), view: () => <Net /> }] },
  { id: "stress", title: "Stress", tabs: [
    { id: "cpu", label: "CPU e RAM", blurb: tr("Leva CPU e RAM ao limite e para sozinho se esquentar demais."), view: () => <Stress /> },
    { id: "gpu", label: "GPU", blurb: tr("Leva a placa de vídeo ao limite por compute shader."), view: () => <GpuStress /> }] },
  { id: "bench", title: "Benchmarks", tabs: [
    { id: "cpu", label: "CPU", blurb: tr("Mede o processador com um path tracer. Quanto maior a nota, melhor."), view: () => <Bench /> },
    { id: "gpu", label: "GPU", blurb: tr("Cena de jogo em Vulkan: terreno, árvores, sombras e água."), view: () => <BenchGpu /> },
    { id: "rt", label: "Ray tracing", blurb: tr("Path tracing em hardware, com ray queries do Vulkan."), view: () => <RayTracing /> }] },
  { id: "jogos", title: "Jogos", tabs: [
    { id: "jogos", label: "Jogos", blurb: tr("Jogos instalados e o FPS esperado para esta máquina."), view: () => <Games /> },
    { id: "gargalo", label: "Gargalo", blurb: tr("Mostra o que limita o desempenho agora: GPU, CPU, memória ou calor."), view: () => <Bottleneck /> },
    { id: "overlay", label: "Overlay", blurb: tr("Escolha o que aparece sobre o jogo."), view: () => <OverlaySettings /> }] },
  { id: "controle", title: "Ventoinhas e LEDs", tabs: [
    { id: "fans", label: "Ventoinhas", blurb: tr("Velocidade das ventoinhas e cor dos LEDs RGB, no mesmo lugar: ajuste manual quando o hardware permitir."), view: () => <Fans /> },
    { id: "curvas", label: "Curvas", blurb: tr("A velocidade das ventoinhas segue a temperatura da CPU ou da GPU, pela curva que você definir."), view: () => <Curves /> },
    { id: "leds", label: "LEDs", blurb: tr("Cor e efeito dos LEDs RGB da placa-mãe, da memória e dos periféricos, quando o hardware permitir."), view: () => <Leds /> },
    { id: "perfis", label: "Perfis", blurb: tr("Guarde as velocidades e as luzes que você ajustou e reaplique com um clique, ou ao iniciar a sessão."), view: () => <Profiles /> }] },
  { id: "ferramentas", title: "Ferramentas", tabs: [
    { id: "startup", label: "Inicialização", blurb: tr("Programas que abrem junto com a sua sessão."), view: () => <Startup /> },
    { id: "limpeza", label: "Limpeza", blurb: tr("Veja o que ocupa espaço e limpe uma categoria por vez."), view: () => <Cleanup /> },
    { id: "serial", label: "Portas e USB", blurb: tr("Placas Arduino e adaptadores seriais, com diagnóstico de permissão."), view: () => <Serial /> },
    { id: "apps", label: "Instalar apps", blurb: tr("Instale os apps de sempre depois de formatar."), view: () => <Apps /> }] },
  { id: "config", title: "Configurações", tabs: [{ id: "config", label: "Configurações", blurb: tr("Alerta de temperatura, bandeja e idioma, num só lugar."), view: () => <Settings /> }] },
];

type Check = { id: string; title: string; status: "ok" | "warn" | "missing" | "info"; detail: string; fix: { kind: "helper" | "page"; target: string; label: string } | null };
const MARK: Record<Check["status"], string> = { ok: "✓", warn: "!", missing: "✕", info: "i" };

function Setup({ onIssues, go }: { onIssues: (n: number) => void; go: (page: string) => void }) {
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const run = () => { setBusy(true); setErr(""); invoke<Check[]>("system_check").then((c) => { setChecks(c); onIssues(c.filter((x) => x.status === "warn" || x.status === "missing").length); }).catch((e) => setErr(String(e))).finally(() => setBusy(false)); };
  useEffect(run, []);
  const fix = (c: Check) => {
    if (!c.fix) return;
    if (c.fix.kind === "page") { go(c.fix.target); return; }
    if (!window.confirm(tf("{0}?\n\n{1}\n\nO sistema vai pedir a sua senha.", tr(c.fix.label), tr(c.detail)))) return;
    setBusy(true);
    invoke<string>("system_fix", { target: c.fix.target }).then((m) => setMsg(m.split("\n").slice(-6).join(" · "))).catch((e) => setMsg(String(e))).finally(() => { setBusy(false); run(); });
  };
  const bad = checks?.filter((c) => c.status === "warn" || c.status === "missing").length ?? 0;
  return (
    <>
      <p className="dim">{err ? tr("Não foi possível verificar o PC. Clique em Verificar de novo; se persistir, reinicie o app.") : checks == null ? tr("Verificando…") : bad === 0 ? tr("Tudo certo: o CoreForge encontrou o que precisa neste PC.") : tf("{0} item(ns) pedem atenção. As verificações só leem; qualquer correção pede confirmação e a senha do sistema.", bad)}</p>
      <section className="card">
        {checks?.map((c) => (
          <div className={`check-row st-${c.status}`} key={c.id}>
            <span className="mark">{MARK[c.status]}</span>
            <div><b>{tr(c.title)}</b><div className="dim">{tr(c.detail)}</div></div>
            {c.fix && <button className="btn small" disabled={busy} onClick={() => fix(c)}>{tr(c.fix.label)}</button>}
          </div>
        ))}
      </section>
      {msg && <p className="dim">{msg}</p>}
      <button className="btn small" disabled={busy} onClick={run}>{tr("Verificar de novo")}</button>
    </>
  );
}

// Escala térmica: azul-gelo (frio) até laranja-brasa (quente). É a única cor "viva" da interface.
// Fica verde-limão até 60% e esquenta até vermelho entre 60% e 85%: o alerta só aparece quando importa.
const heat = (t: number) => { const k = Math.min(Math.max((t - 0.6) / 0.25, 0), 1) * 100; return k === 0 ? "#b4ff1f" : `color-mix(in oklab, #b4ff1f ${100 - k}%, #ff5a36 ${k}%)`; };

const PIXEL_CHIP = (() => {
  const px: [number, number][] = [];
  for (let i = 3; i <= 11; i++) { px.push([i, 3], [i, 11], [3, i], [11, i]); } // corpo
  for (const k of [5, 7, 9]) { px.push([k, 1], [k, 2], [k, 12], [k, 13], [1, k], [2, k], [12, k], [13, k]); } // pinos
  ["...#...", "..###..", ".#####.", ".##.##.", ".#####.", "..###.."].forEach((row, r) => [...row].forEach((c, x) => c === "#" && px.push([x + 4, r + 4])));
  return px;
})();

function PixelMark() {
  return (
    <svg className="pixel" viewBox="0 0 15 15" shapeRendering="crispEdges" aria-hidden>
      {PIXEL_CHIP.map(([x, y], i) => <rect key={i} x={x} y={y} width="1" height="1" />)}
    </svg>
  );
}

type Curve = { kind: string; hw: string; pwm: number; chip: string; label: string; source: "cpu" | "gpu"; points: [number, number][] };
const PRESETS: Record<string, [number, number][]> = {
  "Silencioso": [[40, 25], [60, 35], [75, 60], [85, 100]],
  "Equilibrado": [[40, 30], [55, 45], [70, 70], [80, 100]],
  "Desempenho": [[35, 40], [50, 60], [65, 85], [75, 100]],
};

function CurvePreview({ points }: { points: [number, number][] }) {
  const pts = extendCurve(points);
  return (
    <svg viewBox="0 0 220 110" className="curve-svg" role="img" aria-label={tr("Curva de velocidade")}>
      <path d="M8 8V102H212" fill="none" stroke="var(--line-2)" />
      <polyline points={pts.map(([t, p]) => `${x(t)},${y(p)}`).join(" ")} fill="none" stroke="var(--accent)" strokeWidth="2" />
      {points.map(([t, p], i) => <circle key={i} cx={x(t)} cy={y(p)} r="3.5" fill="var(--accent)" />)}
    </svg>
  );
}

/** Curvas: a velocidade da ventoinha segue a temperatura. Um motor com privilégios (uma senha) aplica enquanto o app está aberto. */
function Curves() {
  const [chans, setChans] = useState<Channel[]>([]);
  const [curves, setCurves] = useState<Curve[]>([]);
  const [st, setSt] = useState<{ running: boolean; error: string }>({ running: false, error: "" });
  const [msg, setMsg] = useState("");
  useEffect(() => {
    invoke<{ channels: Channel[] }>("fans_report").then((r) => setChans(r.channels.filter((c) => c.controllable && !fanOff(c))));
    invoke<Curve[]>("curves_get").then(setCurves);
    let alive = true;
    const tick = () => invoke<{ running: boolean; error: string }>("curves_status").then((x) => alive && setSt(x));
    tick();
    const t = setInterval(tick, 2000);
    return () => { alive = false; clearInterval(t); };
  }, []);
  const same = (c: Channel) => (k: Curve) => k.kind === c.kind && k.chip === c.chip && k.pwm === c.pwm;
  const edit = (c: Channel, f: (k: Curve) => Curve | null) => setCurves((all) => all.flatMap((k) => (same(c)(k) ? (f(k) ? [f(k)!] : []) : [k])));
  const create = (c: Channel) => setCurves((all) => [...all, { kind: c.kind, hw: c.hw, pwm: c.pwm, chip: c.chip, label: c.label, source: c.kind === "gpu" ? "gpu" : "cpu", points: PRESETS["Equilibrado"] }]);
  const save = () => invoke("curves_save", { curves }).then(() => { setMsg(tr("Curvas salvas.")); return true; }).catch((e) => { setMsg(tr(String(e))); return false; });
  const start = async () => { if (!window.confirm(tr("Iniciar as curvas? As ventoinhas passam a seguir a temperatura e o sistema vai pedir a sua senha. Fechar o app devolve o controle ao automático."))) return; if (await save()) invoke("curves_start").then(() => invoke<{ running: boolean; error: string }>("curves_status").then(setSt)).catch((e) => setMsg(tr(String(e)))); };
  const num = (v: string) => Number(v) || 0;
  return (
    <>
      <section className="card">
        <h2>{tr("Motor de curvas")}</h2>
        <p className="dim">{tr("Pede a senha uma vez e aplica as curvas enquanto o CoreForge estiver aberto (use \"Manter na bandeja ao fechar\" em Sensores para deixá-lo rodando). Ao parar ou fechar, as ventoinhas voltam ao automático. Se a temperatura não puder ser lida, a ventoinha vai a 100 %.")}</p>
        <div className="row">
          <span className={st.running ? "strong" : "dim"}>{st.running ? tr("Ligado") : tr("Desligado")}</span>
          {st.running
            ? <button className="btn small danger" onClick={() => invoke("curves_stop")}>{tr("Parar")}</button>
            : <button className="btn small" disabled={curves.length === 0} onClick={start}>{tr("Salvar e ligar")}</button>}
          <button className="btn small ghost" onClick={save}>{tr("Salvar")}</button>
        </div>
        {(msg || st.error) && <p className={st.error ? "err" : "dim"}>{st.error ? tr(st.error) : msg}</p>}
      </section>
      {chans.length === 0 && <p className="dim">{tr("Nenhuma ventoinha controlável encontrada. Veja a aba Ventoinhas.")}</p>}
      <div className="cards cards-2">
        {chans.map((c) => {
          const k = curves.find(same(c));
          return (
            <section key={c.id} className="card">
              <h2>{c.label}</h2>
              {!k ? <button className="btn small" onClick={() => create(c)}>{tr("Criar curva")}</button> : (
                <>
                  <CurvePreview points={k.points} />
                  <div className="row">
                    <label className="dim">{tr("Temperatura de")}{" "}
                      <select value={k.source} onChange={(e) => edit(c, (x) => ({ ...x, source: e.target.value as "cpu" | "gpu" }))}><option value="cpu">CPU</option><option value="gpu">GPU</option></select>
                    </label>
                    {Object.keys(PRESETS).map((n) => <button key={n} className="btn small ghost" onClick={() => edit(c, (x) => ({ ...x, points: PRESETS[n] }))}>{tr(n)}</button>)}
                  </div>
                  {k.points.map(([t, p], i) => (
                    <div key={i} className="kv">
                      <span><input type="number" min={0} max={110} value={t} style={{ width: 64 }} aria-label={tr("Temperatura (°C)")}
                        onChange={(e) => edit(c, (x) => ({ ...x, points: x.points.map((q, j) => (j === i ? [num(e.target.value), q[1]] : q)) }))} /> °C</span>
                      <span><input type="number" min={25} max={100} value={p} style={{ width: 64 }} aria-label={tr("Velocidade (%)")}
                        onChange={(e) => edit(c, (x) => ({ ...x, points: x.points.map((q, j) => (j === i ? [q[0], num(e.target.value)] : q)) }))} /> %
                        {k.points.length > 2 && <button className="btn small ghost" aria-label={tr("Remover ponto")} onClick={() => edit(c, (x) => ({ ...x, points: x.points.filter((_, j) => j !== i) }))}>×</button>}</span>
                    </div>
                  ))}
                  <div className="row">
                    {k.points.length < 8 && <button className="btn small ghost" onClick={() => edit(c, (x) => { const l = x.points[x.points.length - 1]; return { ...x, points: [...x.points, [Math.min(110, l[0] + 5), 100]] }; })}>{tr("Adicionar ponto")}</button>}
                    <button className="btn small danger" onClick={() => edit(c, () => null)}>{tr("Remover curva")}</button>
                  </div>
                </>
              )}
            </section>
          );
        })}
      </div>
    </>
  );
}

type Battery = { name: string; status: string; percent: number | null; health_pct: number | null; power_w: number | null; cycles: number | null; limit_pct: number | null; technology: string };
type PowerInfo = { batteries: Battery[]; ac_online: boolean | null; profile: string; profiles: string[] };
const BAT_STATUS: Record<string, string> = { Charging: "Carregando", Discharging: "Descarregando", Full: "Carregada", "Not charging": "Parada" };
const PROFILE_LABEL: Record<string, string> = { "power-saver": "Economia", balanced: "Equilibrado", performance: "Desempenho" };

/** Bateria e perfil de energia: pensado para notebooks, mas mostra o perfil também em desktops que tenham o serviço. */
function Power() {
  const [p, setP] = useState<PowerInfo | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    let alive = true;
    const tick = () => invoke<PowerInfo>("power_info").then((r) => alive && setP(r));
    tick();
    const t = setInterval(tick, 3000);
    return () => { alive = false; clearInterval(t); };
  }, []);
  if (!p) return <Loading />;
  const setProfile = (name: string) => { setErr(""); invoke("power_profile_set", { name }).then(() => invoke<PowerInfo>("power_info").then(setP)).catch((e) => setErr(String(e))); };
  const row = (k: string, v: string) => <div className="kv" key={k}><span className="dim">{tr(k)}</span><span className="strong">{v}</span></div>;
  return (
    <>
      {p.batteries.length === 0 && <p className="dim">{tr("Nenhuma bateria encontrada: este parece ser um desktop.")}</p>}
      <div className="cards cards-3">
        {p.batteries.map((b) => (
          <section key={b.name} className="card">
            <h2>{tr("Bateria")} {p.batteries.length > 1 ? b.name : ""}</h2>
            <Meter label={tr(BAT_STATUS[b.status] ?? (b.status || "—"))} value={b.percent} text={b.percent != null ? `${b.percent.toFixed(0)} %` : "—"} />
            {row("Fonte", p.ac_online == null ? "—" : p.ac_online ? tr("Na tomada") : tr("Na bateria"))}
            {b.power_w != null && row(b.status === "Charging" ? "Carregando a" : "Consumo da bateria", `${b.power_w.toFixed(1)} W`)}
            {b.health_pct != null && row("Saúde", `${b.health_pct.toFixed(0)} %`)}
            {b.cycles != null && row("Ciclos de carga", String(b.cycles))}
            {b.limit_pct != null && row("Limite de carga", `${b.limit_pct} %`)}
            {b.technology && row("Tecnologia", b.technology)}
          </section>
        ))}
        {p.profiles.length > 0 && (
          <section className="card">
            <h2>{tr("Perfil de energia")}</h2>
            <div className="row">
              {p.profiles.map((n) => <button key={n} className={`btn small ${n === p.profile ? "" : "ghost"}`} aria-pressed={n === p.profile} onClick={() => setProfile(n)}>{tr(PROFILE_LABEL[n] ?? n)}</button>)}
            </div>
            {err && <p className="err">{tr(err)}</p>}
          </section>
        )}
      </div>
      {p.batteries.length === 0 && p.profiles.length === 0 && <p className="dim">{tr("Perfis de energia indisponíveis: instale o power-profiles-daemon.")}</p>}
    </>
  );
}

function Meter({ label, value, text }: { label: string; value: number | null; text: string }) {
  const t = value == null ? 0 : value / 100;
  return (
    <div className="meter" title={`${label}: ${text}`}>
      <div className="meter-top"><span>{label}</span><b>{text}</b></div>
      <div className="meter-bar"><i style={{ width: `${Math.max(2, t * 100)}%`, background: heat(t) }} /></div>
    </div>
  );
}

function ThermalStrip() {
  const m = useMetrics(1500);
  const hot = useRef<Record<string, number>>({});
  // Alerta: 3 leituras seguidas (~5 s) no limite ou acima; rearma quando cai 5 °C abaixo, para não repetir o aviso.
  useEffect(() => {
    if (!m) return;
    const limit = tempLimit();
    for (const [name, temp] of [["CPU", m.cpu_temp], ["GPU", m.gpu_temp]] as const) {
      if (!limit || temp == null || temp < limit - 5) { hot.current[name] = 0; continue; }
      if (temp < limit) continue;
      hot.current[name] = (hot.current[name] ?? 0) + 1;
      if (hot.current[name] === 3) {
        const msg = tf("{0} a {1} °C (limite: {2} °C). Verifique a refrigeração.", name, Math.round(temp), limit);
        window.dispatchEvent(new CustomEvent("coreforge-alert", { detail: msg }));
        invoke("notify", { title: tr("Temperatura alta"), body: msg }).catch(() => {});
      }
    }
  }, [m]);
  const pct = (a: number | null, b: number | null) => (a != null && b ? (100 * a) / b : null);
  useEffect(() => {
    if (!m) return;
    pushSample({ t: Date.now(), cpu_load: m.cpu_load, cpu_busiest: m.cpu_busiest, cpu_temp: m.cpu_temp, gpu_util: m.gpu_util, gpu_temp: m.gpu_temp, gpu_power: m.gpu_power_w, fan_rpm: m.fan_rpm,
      ram_pct: pct(m.ram_used_gb, m.ram_total_gb), vram_pct: pct(m.vram_used_mb, m.vram_total_mb) });
  }, [m]);
  return (
    <div className="strip" aria-label={tr("Estado ao vivo")}>
      <Meter label="CPU" value={m?.cpu_load ?? null} text={m ? `${(m.cpu_load ?? 0).toFixed(0)} % · ${n0(m.cpu_temp, "°C")}` : "—"} />
      <Meter label="GPU" value={m?.gpu_util ?? null} text={m ? `${Math.round(m.gpu_util ?? 0)} % · ${n0(m.gpu_temp, "°C")}` : "—"} />
      <Meter label="VRAM" value={pct(m?.vram_used_mb ?? null, m?.vram_total_mb ?? null)} text={m?.vram_used_mb != null ? `${(m.vram_used_mb / 1024).toFixed(1)} GB` : "—"} />
      <Meter label="RAM" value={pct(m?.ram_used_gb ?? null, m?.ram_total_gb ?? null)} text={m?.ram_used_gb != null ? `${m.ram_used_gb.toFixed(1)} GB` : "—"} />
    </div>
  );
}

/** Evita que um erro numa página derrube o app inteiro: mostra o erro e permite tentar de novo. */
class Boundary extends Component<{ children: ReactNode; resetKey: number }, { err: Error | null }> {
  state = { err: null as Error | null };
  static getDerivedStateFromError(err: Error) { return { err }; }
  componentDidUpdate(prev: { resetKey: number }) { if (prev.resetKey !== this.props.resetKey && this.state.err) this.setState({ err: null }); }
  render() {
    if (!this.state.err) return this.props.children;
    return (
      <section className="card">
        <p className="err">{tr("Esta página falhou ao carregar.")}</p>
        <pre className="log">{String(this.state.err.message)}</pre>
        <button className="btn small" onClick={() => this.setState({ err: null })}>{tr("Tentar de novo")}</button>
      </section>
    );
  }
}

export default function App() {
  const [sec, setSec] = useState(0);
  const [tabs, setTabs] = useState<Record<string, string>>(() => { try { return JSON.parse(localStorage.getItem("tabs") ?? "{}"); } catch { return {}; } });
  const [issues, setIssues] = useState(0);
  const [fail, setFail] = useState("");
  // Falhas de comandos que ninguém tratou viram um aviso na tela, em vez de passarem em silêncio.
  useEffect(() => {
    const h = (e: PromiseRejectionEvent) => { setFail(String(e.reason?.message ?? e.reason)); e.preventDefault(); };
    const a = (e: Event) => setFail((e as CustomEvent<string>).detail);
    window.addEventListener("unhandledrejection", h);
    window.addEventListener("coreforge-alert", a);
    return () => { window.removeEventListener("unhandledrejection", h); window.removeEventListener("coreforge-alert", a); };
  }, []);
  // Verificação automática ao abrir: só lê; o resultado vira um aviso no menu.
  useEffect(() => { if (localStorage.getItem("keepTray") === "1") invoke("tray_close_set", { on: true }).catch(() => {}); }, []);
  useEffect(() => { invoke<Check[]>("system_check").then((c) => setIssues(c.filter((x) => x.status === "warn" || x.status === "missing").length)).catch(() => {}); }, []);
  if (new URLSearchParams(location.search).has("overlay")) return <OverlayView />;
  const section = SECTIONS[sec];
  const tabIdx = Math.max(0, section.tabs.findIndex((t) => t.id === tabs[section.id]));
  const tab = section.tabs[tabIdx];
  const pickTab = (id: string) => setTabs((t) => { const n = { ...t, [section.id]: id }; localStorage.setItem("tabs", JSON.stringify(n)); return n; });
  const goLeds = () => { const i = SECTIONS.findIndex((x) => x.id === "controle"); setTabs((t) => { const n = { ...t, controle: "leds" }; localStorage.setItem("tabs", JSON.stringify(n)); return n; }); setSec(i); };
  const words = tr(section.title).split(" ");
  return (
    <div className="shell">
      <Welcome onSetup={() => setSec(0)} />
      <nav aria-label={tr("Seções")}>
        <div className="brand">
          <div className="tile"><PixelMark /><span>coreforge · online</span></div>
          <div className="word"><img src={logo} alt="" />CoreForge</div>
        </div>
        <div className="nav-list">
          {SECTIONS.map((x, i) => (
            <Fragment key={x.id}>
              {NAV_GROUP[x.id] && NAV_GROUP[x.id] !== NAV_GROUP[SECTIONS[i - 1]?.id] && <div className="nav-group-name">{tr(NAV_GROUP[x.id])}</div>}
              <button className={i === sec ? "active" : ""} aria-current={i === sec ? "page" : undefined} onClick={() => setSec(i)}>
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d={NAV_ICON[x.id]} /></svg>
                <span>{tr(x.title)}</span>{x.id === "setup" && issues > 0 && <span className="badge-n" aria-label={tf("{0} avisos", issues)}>{issues}</span>}
              </button>
            </Fragment>
          ))}
        </div>
      </nav>
      <main>
        <header className="top">
          <div>
            <span className="pill"><i />{tr("ao vivo")}</span>
            <h1>{words.slice(0, -1).join(" ")} <em>{words.slice(-1)}</em><u>_</u></h1>
            <p className="blurb">{tab.blurb}</p>
          </div>
          <ThermalStrip />
        </header>
        {fail && <div className="toast" role="alert"><span>{tr(fail)}</span><button onClick={() => setFail("")} aria-label={tr("Fechar")}>×</button></div>}
        {section.tabs.length > 1 && (
          <div className="tabs" role="tablist" aria-label={tr(section.title)} onKeyDown={(e) => {
            const d = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : e.key === "Home" ? -tabIdx : e.key === "End" ? section.tabs.length : 0;
            if (!d) return;
            e.preventDefault(); const n = section.tabs[(tabIdx + d + section.tabs.length) % section.tabs.length].id; pickTab(n);
            requestAnimationFrame(() => document.getElementById(`tab-${n}`)?.focus());
          }}>
            {section.tabs.map((t) => <button key={t.id} id={`tab-${t.id}`} role="tab" aria-selected={t.id === tab.id} aria-controls="tabpanel" tabIndex={t.id === tab.id ? 0 : -1} className={t.id === tab.id ? "active" : ""} onClick={() => pickTab(t.id)}>{tr(t.label)}</button>)}
          </div>
        )}
        <Boundary resetKey={sec * 100 + tabIdx}>
          <div className="page" id="tabpanel" role={section.tabs.length > 1 ? "tabpanel" : undefined} aria-labelledby={section.tabs.length > 1 ? `tab-${tab.id}` : undefined} key={`${section.id}-${tab.id}`}>{tab.view({ setIssues, goLeds })}</div>
        </Boundary>
      </main>
    </div>
  );
}
