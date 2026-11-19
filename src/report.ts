import { loadRuns, RUN_KINDS } from "./runs";
import { t as tr } from "./i18n";

type Node = { header: string; value: string; children: Node[] };

/** Relatório em Markdown: informações do sistema e o último resultado de cada benchmark. Pronto para colar num fórum ou pedido de suporte. */
export function buildReport(nodes: Node[], version = "0.1.0"): string {
  const lines = [`# CoreForge ${version} — ${new Date().toLocaleString()}`, ""];
  const walk = (n: Node, depth: number) => {
    if (n.children.length === 0) { lines.push(`${"  ".repeat(depth)}- ${tr(n.header)}: ${tr(n.value) || "—"}`); return; }
    lines.push(`${"  ".repeat(depth)}- **${tr(n.header)}**${n.value ? `: ${n.value}` : ""}`);
    n.children.forEach((c) => walk(c, depth + 1));
  };
  for (const n of nodes) { lines.push(`## ${tr(n.header)}`); n.children.forEach((c) => walk(c, 0)); lines.push(""); }
  const runs = Object.entries(RUN_KINDS).map(([k, v]) => ({ k, v, last: loadRuns(k)[0] })).filter((r) => r.last);
  if (runs.length) {
    lines.push(`## ${tr("Benchmarks")}`);
    for (const { v, last } of runs) lines.push(`- ${tr(v.title)}: **${Math.round(last.value)} ${v.unit}** (${last.detail}) — ${new Date(last.at).toLocaleString()}`);
    lines.push("");
  }
  return lines.join("\n");
}
