// Histórico recente das leituras: alimentado pela faixa do topo (sempre ligada) e lido pelos gráficos de qualquer aba.
export type Sample = { t: number; cpu_load: number | null; cpu_busiest: number | null; cpu_temp: number | null; gpu_util: number | null;
  gpu_temp: number | null; gpu_power: number | null; fan_rpm: number | null; ram_pct: number | null; vram_pct: number | null };
export type SeriesKey = Exclude<keyof Sample, "t">;
export const MAX_SAMPLES = 400; // ~10 min a cada 1,5 s
export const hist: Sample[] = [];
export function pushSample(s: Sample) { hist.push(s); if (hist.length > MAX_SAMPLES) hist.shift(); }
