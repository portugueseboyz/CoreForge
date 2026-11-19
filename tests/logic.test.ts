import { beforeEach, describe, expect, it, vi } from "vitest";
import { curveX, curveY, extendCurve, tempLimit } from "../src/curvemath";

const store = (v: string | null) => ({ getItem: () => v });

describe("tempLimit", () => {
  it("defaults to 90 when unset", () => expect(tempLimit(store(null))).toBe(90));
  it("reads a number, 0 disables, garbage becomes 0", () => {
    expect(tempLimit(store("75"))).toBe(75);
    expect(tempLimit(store("0"))).toBe(0);
    expect(tempLimit(store("abc"))).toBe(0);
    expect(tempLimit(store(""))).toBe(0);
  });
});

describe("curve preview math", () => {
  it("maps axis corners", () => {
    expect(curveX(20)).toBe(8);
    expect(curveX(110)).toBe(212);
    expect(curveY(0)).toBe(102);
    expect(curveY(100)).toBe(8);
  });
  it("extends to the borders with first/last duty", () => {
    expect(extendCurve([[40, 30], [80, 100]])).toEqual([[20, 30], [40, 30], [80, 100], [110, 100]]);
    expect(extendCurve([])).toEqual([[20, 0], [110, 0]]);
  });
});

/** i18n reads localStorage/navigator at import time, so each case loads a fresh module. */
async function load(lang: "pt" | "en") {
  vi.resetModules();
  vi.stubGlobal("localStorage", { getItem: (k: string) => (k === "lang" ? lang : null), setItem() {}, removeItem() {} });
  return import("../src/i18n");
}

describe("i18n", () => {
  beforeEach(() => vi.unstubAllGlobals());
  it("pt returns the key untouched", async () => {
    const { t, tf } = await load("pt");
    expect(t("Preferências")).toBe("Preferências");
    expect(tf("{0} canais", 4)).toBe("4 canais");
  });
  it("en translates exact keys, falls back to pt when missing, keeps empty", async () => {
    const { t } = await load("en");
    expect(t("Preferências")).toBe("Preferences");
    expect(t("texto que não existe")).toBe("texto que não existe");
    expect(t("")).toBe("");
  });
  it("en matches templates and translates captured parts", async () => {
    const { t } = await load("en");
    expect(t("{0} a {1} °C (limite: {2} °C). Verifique a refrigeração.".replace("{0}", "CPU").replace("{1}", "95").replace("{2}", "90"))).toBe("CPU at 95 °C (limit: 90 °C). Check your cooling.");
  });
  it("tf leaves a missing value empty", async () => {
    const { tf } = await load("pt");
    expect(tf("a {0} b {1}", "x")).toBe("a x b ");
  });
});

describe("buildReport", () => {
  it("renders sections, leaf and group nodes, and last runs", async () => {
    vi.resetModules();
    const mem: Record<string, string> = {};
    vi.stubGlobal("localStorage", { getItem: (k: string) => (k === "lang" ? "pt" : mem[k] ?? null), setItem: (k: string, v: string) => { mem[k] = v; } });
    const { saveRun } = await import("../src/runs");
    saveRun("bench-cpu", { value: 1234.4, detail: "8 threads", sig: "a" });
    const { buildReport } = await import("../src/report");
    const md = buildReport([{ header: "Sistema", value: "", children: [
      { header: "Kernel", value: "6.1", children: [] },
      { header: "Grupo", value: "v", children: [{ header: "Filho", value: "", children: [] }] },
    ] }], "9.9");
    expect(md).toContain("# CoreForge 9.9 — ");
    expect(md).toContain("## Sistema\n- Kernel: 6.1\n- **Grupo**: v\n  - Filho: —");
    expect(md).toContain("## Benchmarks\n- Bench CPU: **1234 pts** (8 threads) — ");
  });
  it("omits Benchmarks when there are no runs", async () => {
    vi.resetModules();
    vi.stubGlobal("localStorage", { getItem: () => null, setItem() {} });
    const { buildReport } = await import("../src/report");
    expect(buildReport([])).not.toContain("Benchmarks");
  });
});
