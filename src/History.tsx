import { useEffect, useState } from "react";
import { setInterval } from "./poll";
import { hist, type Sample, type SeriesKey } from "./history";
import { t as tr, tf } from "./i18n";

const W = 600, H = 170, L = 40, R = 10, T = 10, B = 22, WINDOW = 600_000;
type Ser = { key: SeriesKey; label: string; color: string };
const CPU = "#b4ff1f", GPU = "#ff5a36";
const GROUPS: { title: string; unit: string; min?: number; max?: number; series: Ser[] }[] = [
  { title: "Temperaturas", unit: "°C", series: [{ key: "cpu_temp", label: "CPU", color: CPU }, { key: "gpu_temp", label: "GPU", color: GPU }] },
  { title: "Carga", unit: "%", min: 0, max: 100, series: [{ key: "cpu_load", label: "CPU", color: CPU }, { key: "gpu_util", label: "GPU", color: GPU }] },
  { title: "Consumo da GPU", unit: "W", min: 0, series: [{ key: "gpu_power", label: "GPU", color: GPU }] },
  { title: "Ventoinha", unit: "RPM", min: 0, series: [{ key: "fan_rpm", label: "RPM", color: CPU }] },
];

function stats(data: Sample[], k: SeriesKey) {
  const v = data.map((s) => s[k]).filter((x): x is number => x != null);
  return v.length ? { min: Math.min(...v), avg: v.reduce((a, b) => a + b, 0) / v.length, max: Math.max(...v) } : null;
}

function HistChart({ g, data }: { g: (typeof GROUPS)[number]; data: Sample[] }) {
  const [cur, setCur] = useState<number | null>(null);
  const vals = g.series.flatMap((s) => data.map((d) => d[s.key]).filter((v): v is number => v != null));
  const lo = g.min ?? (vals.length ? Math.floor(Math.min(...vals) / 10) * 10 : 0);
  const hi = g.max ?? (vals.length ? Math.max(lo + 10, Math.ceil(Math.max(...vals) / 10) * 10) : 100);
  const end = data.length ? data[data.length - 1].t : 0;
  const span = Math.max(60_000, data.length ? Math.min(WINDOW, end - data[0].t) : 0);
  const px = (t: number) => L + (W - L - R) * (1 - (end - t) / span);
  const py = (v: number) => T + (H - T - B) * (1 - (Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo || 1));
  const pick = (clientX: number, r: DOMRect) => {
    if (data.length < 2) return;
    const x = ((clientX - r.left) / r.width) * W;
    let best = 0;
    data.forEach((d, i) => { if (Math.abs(px(d.t) - x) < Math.abs(px(data[best].t) - x)) best = i; });
    setCur(best);
  };
  const sel = cur != null && cur < data.length ? data[cur] : null;
  const has = vals.length > 0;
  return (
    <section className="card chart hist">
      <div className="chart-head"><h3>{tr(g.title)}</h3>
        <div className="legend">{g.series.map((s) => <span key={s.key}><i style={{ background: s.color }} />{s.label}</span>)}</div></div>
      {has ? (
        <>
          <svg viewBox={`0 0 ${W} ${H}`} role="img" tabIndex={0} aria-label={tr(g.title)} onPointerMove={(e) => pick(e.clientX, e.currentTarget.getBoundingClientRect())}
            onPointerDown={(e) => pick(e.clientX, e.currentTarget.getBoundingClientRect())} onPointerLeave={(e) => e.pointerType === "mouse" && setCur(null)}
            onKeyDown={(e) => { if (e.key === "ArrowLeft") setCur(Math.max(0, (cur ?? data.length) - 1)); else if (e.key === "ArrowRight") setCur(Math.min(data.length - 1, (cur ?? -1) + 1)); else if (e.key === "Escape") setCur(null); }}>
            {[0, 0.5, 1].map((f) => { const v = lo + (hi - lo) * f; return <g key={f}><line x1={L} x2={W - R} y1={py(v)} y2={py(v)} className="grid" /><text x={L - 5} y={py(v) + 3} className="axis" textAnchor="end">{Math.round(v)}</text></g>; })}
            {g.series.map((s) => { const pts = data.filter((d) => d[s.key] != null).map((d) => `${px(d.t).toFixed(1)},${py(d[s.key] as number).toFixed(1)}`).join(" ");
              return pts && <polyline key={s.key} points={pts} fill="none" stroke={s.color} strokeWidth="1.8" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />; })}
            {sel && <g className="hist-cursor"><line x1={px(sel.t)} x2={px(sel.t)} y1={T} y2={H - B} />{g.series.map((s) => sel[s.key] != null && <circle key={s.key} cx={px(sel.t)} cy={py(sel[s.key] as number)} r="3.5" fill={s.color} />)}</g>}
            <text x={L} y={H - 5} className="axis">{tf("há {0} min", Math.max(1, Math.round(span / 60_000)))}</text><text x={W - R} y={H - 5} className="axis" textAnchor="end">{tr("agora")}</text>
          </svg>
          <div className="hist-read" aria-live="off">{sel
            ? <>{tf("há {0} s", Math.round((end - sel.t) / 1000))} · {g.series.map((s) => <b key={s.key} style={{ color: s.color }}>{s.label} {sel[s.key] == null ? "—" : `${Math.round(sel[s.key] as number)} ${g.unit}`}{" "}</b>)}</>
            : <span className="dim">{tr("Passe o mouse ou toque no gráfico para ver o valor.")}</span>}</div>
          <table className="hist-stats"><thead><tr><th /><th>{tr("mín.")}</th><th>{tr("média")}</th><th>{tr("máx.")}</th></tr></thead><tbody>
            {g.series.map((s) => { const st = stats(data, s.key); const f = (v?: number) => (v == null ? "—" : `${Math.round(v)} ${g.unit}`);
              return <tr key={s.key}><th scope="row"><i style={{ background: s.color }} />{s.label}</th><td>{f(st?.min)}</td><td>{f(st?.avg)}</td><td>{f(st?.max)}</td></tr>; })}</tbody></table>
        </>
      ) : <p className="dim">{data.length ? tr("Sem sensor para esta leitura neste PC.") : tr("Coletando dados…")}</p>}
    </section>
  );
}

/** Últimos 10 min das leituras da faixa do topo (sem novo polling). Pausar congela a vista; a coleta continua. */
export function History() {
  const [, force] = useState(0);
  const [frozen, setFrozen] = useState<Sample[] | null>(null);
  useEffect(() => { const t = setInterval(() => force((n) => n + 1), 1500); return () => clearInterval(t); }, []);
  const last = hist.length ? hist[hist.length - 1].t : 0;
  const data = frozen ?? hist.filter((s) => s.t >= last - WINDOW);
  return (
    <>
      <div className="row hist-bar">
        <button className="btn small" aria-pressed={frozen != null} onClick={() => setFrozen(frozen ? null : [...data])}>{frozen ? tr("Retomar") : tr("Pausar")}</button>
        <button className="btn small" onClick={() => { hist.length = 0; setFrozen(null); force((n) => n + 1); }}>{tr("Limpar")}</button>
        <span className="dim" role="status">{frozen ? tr("Vista pausada: a coleta continua em segundo plano.") : tf("{0} leituras na memória", data.length)}</span>
      </div>
      <div className="cards">{GROUPS.map((g) => <HistChart key={g.title} g={g} data={data} />)}</div>
    </>
  );
}
