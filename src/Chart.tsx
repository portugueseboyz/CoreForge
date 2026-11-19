import { useEffect, useState } from "react";
import { setInterval } from "./poll";
import { hist, type SeriesKey } from "./history";
import { t as tr, tf } from "./i18n";

export type Line = { key: SeriesKey; label: string; color: string };

const W = 600, H = 150, L = 38, R = 8, T = 8, B = 20;

/** Gráfico de linhas em SVG puro, sem dependências: eixo fixo (min/max) ou automático. */
export function Chart({ title, lines, unit, min, max }: { title: string; lines: Line[]; unit: string; min?: number; max?: number }) {
  const [, force] = useState(0);
  useEffect(() => { const t = setInterval(() => force((n) => n + 1), 1500); return () => clearInterval(t); }, []);
  const vals = lines.flatMap((l) => hist.map((s) => s[l.key]).filter((v): v is number => v != null));
  const lo = min ?? (vals.length ? Math.floor(Math.min(...vals) / 10) * 10 : 0);
  const hi = max ?? (vals.length ? Math.max(lo + 10, Math.ceil(Math.max(...vals) / 10) * 10) : 100);
  // Eixo do tempo: da primeira leitura até agora, com no mínimo 1 minuto de largura (o gráfico vai enchendo da direita).
  const now = hist.length ? hist[hist.length - 1].t : Date.now();
  const span = Math.max(60_000, hist.length ? now - hist[0].t : 0);
  const px = (i: number) => L + ((W - L - R) * (1 - (now - hist[i].t) / span));
  const spanMin = Math.max(1, Math.round(span / 60_000));
  const py = (v: number) => T + (H - T - B) * (1 - (Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo || 1));
  const path = (k: SeriesKey) => hist.map((s, i) => (s[k] == null ? null : `${px(i).toFixed(1)},${py(s[k] as number).toFixed(1)}`)).filter(Boolean).join(" ");
  const last = (k: SeriesKey) => { for (let i = hist.length - 1; i >= 0; i--) { const v = hist[i][k]; if (v != null) return v; } return null; };
  const grid = [0, 0.5, 1].map((f) => lo + (hi - lo) * f);
  return (
    <section className="card chart">
      <div className="chart-head">
        <h3>{title}</h3>
        <div className="legend">{lines.map((l) => <span key={l.key}><i style={{ background: l.color }} />{tr(l.label)} <b>{last(l.key) == null ? "—" : `${Math.round(last(l.key) as number)} ${unit}`}</b></span>)}</div>
      </div>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={title}>
        {grid.map((g) => <g key={g}><line x1={L} x2={W - R} y1={py(g)} y2={py(g)} className="grid" /><text x={L - 5} y={py(g) + 3} className="axis" textAnchor="end">{Math.round(g)}</text></g>)}
        {lines.map((l) => hist.length > 1 && <polyline key={l.key} points={path(l.key)} fill="none" stroke={l.color} strokeWidth="1.8" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />)}
        <text x={L} y={H - 4} className="axis">{tf("há {0} min", spanMin)}</text><text x={W - R} y={H - 4} className="axis" textAnchor="end">{tr("agora")}</text>
      </svg>
      {hist.length < 2 && <p className="dim">{tr("Coletando dados…")}</p>}
    </section>
  );
}

const C = { cpu: "#b4ff1f", gpu: "#ff5a36", ram: "#ffb020", vram: "#d0d0d0" };
export const TEMP_LINES: Line[] = [{ key: "cpu_temp", label: "CPU", color: C.cpu }, { key: "gpu_temp", label: "GPU", color: C.gpu }];
export const LOAD_LINES: Line[] = [{ key: "cpu_load", label: "CPU", color: C.cpu }, { key: "gpu_util", label: "GPU", color: C.gpu }, { key: "ram_pct", label: "RAM", color: C.ram }, { key: "vram_pct", label: "VRAM", color: C.vram }];
export const POWER_LINES: Line[] = [{ key: "gpu_power", label: "GPU", color: C.gpu }];
export const BOTTLENECK_LINES: Line[] = [{ key: "gpu_util", label: "GPU", color: C.gpu }, { key: "cpu_busiest", label: "CPU núcleo mais ocupado", color: C.cpu }];
