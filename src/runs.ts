// Histórico dos benchmarks (últimos 10 por tipo), guardado no próprio app.
export type Run = { at: number; value: number; detail: string; sig: string };
export const RUN_KINDS: Record<string, { title: string; unit: string }> = {
  "bench-cpu": { title: "Bench CPU", unit: "pts" }, "bench-gpu": { title: "Bench GPU", unit: "pts" },
  "rt": { title: "Ray tracing", unit: "pts" }, "disk": { title: "Disco (leitura sequencial)", unit: "MB/s" },
};
const key = (k: string) => `runs:${k}`;
export function loadRuns(kind: string): Run[] { try { return JSON.parse(localStorage.getItem(key(kind)) ?? "[]") as Run[]; } catch { return []; } }
/** Guarda um resultado novo (ignora o mesmo resultado repetido) e devolve a lista, do mais novo ao mais antigo. */
export function saveRun(kind: string, r: Omit<Run, "at">): Run[] {
  const list = loadRuns(kind);
  if (list[0]?.sig === r.sig) return list;
  const next = [{ ...r, at: Date.now() }, ...list].slice(0, 10);
  localStorage.setItem(key(kind), JSON.stringify(next));
  return next;
}
