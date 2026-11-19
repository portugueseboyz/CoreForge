// Idiomas: o texto-fonte é o português (o próprio texto é a chave); em inglês, procura no dicionário e cai no português se faltar.
import { EXACT, TEMPLATES } from "./locales/en";

export type Lang = "pt" | "en";
const saved = typeof localStorage !== "undefined" ? (localStorage.getItem("lang") as Lang | null) : null;
export const lang: Lang = saved ?? (typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("pt") ? "pt" : "en");
export const setLang = (l: Lang) => { localStorage.setItem("lang", l); location.reload(); };

const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// Modelos com {0}, {1}…: viram expressões regulares; os trechos capturados também são traduzidos.
const compiled = TEMPLATES.map(([pt, en]) => [new RegExp("^" + esc(pt).replace(/\\\{(\d+)\\\}/g, "(.+?)") + "$", "s"), en] as const);

export function t(pt: string): string {
  if (lang === "pt" || !pt) return pt;
  const hit = EXACT[pt];
  if (hit !== undefined) return hit;
  for (const [re, en] of compiled) {
    const m = pt.match(re);
    if (m) return en.replace(/\{(\d+)\}/g, (_, i) => t(m[Number(i) + 1] ?? ""));
  }
  return pt;
}

/** Texto com partes variáveis: traduz o modelo ("{0} canais") e preenche os valores depois. */
export const tf = (pt: string, ...vals: (string | number)[]) => t(pt).replace(/\{(\d+)\}/g, (_, i) => String(vals[Number(i)] ?? ""));
