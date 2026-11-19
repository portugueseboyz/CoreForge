import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { EXACT, TEMPLATES } from "../src/locales/en";

const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const templates = TEMPLATES.map(([pt]) => new RegExp("^" + esc(pt).replace(/\\\{(\d+)\\\}/g, "(.+?)") + "$", "s"));
const known = (k: string) => k in EXACT || templates.some((re) => re.test(k));

const files = readdirSync("src").filter((f) => /\.tsx?$/.test(f) && f !== "dev-mock.ts").map((f) => `src/${f}`);
/** Literais de string passados direto a tr(...)/t(...)/tf(...). Chamadas com variável ou template literal ficam de fora. */
const used = new Set<string>();
for (const f of files) {
  for (const m of readFileSync(f, "utf8").matchAll(/\b(?:tr|tf|t)\(\s*"((?:[^"\\\n]|\\.)*)"/g)) used.add(JSON.parse(`"${m[1]}"`));
}

describe("i18n keys", () => {
  it("scans something", () => expect(used.size).toBeGreaterThan(50));
  it("every Portuguese key used in code has an EN entry", () => {
    const missing = [...used].filter((k) => !known(k) && /[a-zà-ú]{3}/i.test(k));
    expect(missing).toEqual([]);
  });
  it("no EN value is empty", () => {
    expect(Object.entries(EXACT).filter(([, v]) => !v.trim()).map(([k]) => k)).toEqual([]);
  });
});
