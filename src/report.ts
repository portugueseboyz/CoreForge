import { loadRuns, RUN_KINDS } from "./runs";
import { t as tr } from "./i18n";
import { PRIVATE_KEY, redact } from "./redact";

type Node = { header: string; value: string; children: Node[] };
type Extra = { sensors?: { hardware: string; name: string; category: string; value: number; unit: string }[]; checks?: { title: string; status: string; detail: string }[] };

/** Relatório em Markdown: sistema, sensores, setup e o último resultado de cada benchmark. Só hardware: hostname, serial, MAC, IP e usuário ficam de fora (ver redact.ts). */
export function buildReport(nodes: Node[], version = "0.1.0", extra: Extra = {}): string {
  const secrets: string[] = [];
  const lines = [`# CoreForge ${version} — ${new Date().toLocaleString()}`, ""];
  const walk = (n: Node, depth: number) => {
    if (PRIVATE_KEY.test(n.header)) { if (n.value) secrets.push(n.value); return; }
    if (n.children.length === 0) { lines.push(`${"  ".repeat(depth)}- ${tr(n.header)}: ${tr(n.value) || "—"}`); return; }
    lines.push(`${"  ".repeat(depth)}- **${tr(n.header)}**${n.value ? `: ${n.value}` : ""}`);
    n.children.forEach((c) => walk(c, depth + 1));
  };
  for (const n of nodes) { lines.push(`## ${tr(n.header)}`); n.children.forEach((c) => walk(c, 0)); lines.push(""); }
  if (extra.sensors?.length) {
    lines.push(`## ${tr("Sensores")}`);
    for (const s of extra.sensors) lines.push(`- ${s.hardware} · ${s.name} (${tr(s.category)}): ${s.value.toFixed(s.unit === "MHz" || s.unit === "RPM" ? 0 : 1)} ${s.unit}`);
    lines.push("");
  }
  const runs = Object.entries(RUN_KINDS).map(([k, v]) => ({ k, v, last: loadRuns(k)[0] })).filter((r) => r.last);
  if (runs.length) {
    lines.push(`## ${tr("Benchmarks")}`);
    for (const { v, last } of runs) lines.push(`- ${tr(v.title)}: **${Math.round(last.value)} ${v.unit}** (${last.detail}) — ${new Date(last.at).toLocaleString()}`);
    lines.push("");
  }
  if (extra.checks?.length) {
    lines.push(`## ${tr("Preparação")}`);
    for (const c of extra.checks) lines.push(`- [${c.status}] ${tr(c.title)}: ${tr(c.detail)}`);
    lines.push("");
  }
  return redact(lines.join("\n"), secrets);
}
