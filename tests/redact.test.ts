import test from "node:test";
import assert from "node:assert/strict";
import { redact, PRIVATE_KEY } from "../src/redact.ts";

test("redact removes personal identifiers but keeps hardware text", () => {
  const raw = "Host: maquina-da-ana\nMAC aa:bb:cc:dd:ee:ff ip 192.168.1.226 fe80:0:0:0:1:2:3:4\nana@maquina-da-ana\n/home/ana/.config /run/user/1000\nRyzen 5 5500 PCI 01:00.0 ligado 02:41:12 4269 MHz";
  const out = redact(raw, ["maquina-da-ana"]);
  for (const leak of ["maquina-da-ana", "aa:bb", "192.168", "fe80", "ana@", "/home/ana", "/1000"]) assert.ok(!out.includes(leak), leak);
  for (const keep of ["Ryzen 5 5500", "01:00.0", "02:41:12", "4269 MHz"]) assert.ok(out.includes(keep), keep);
});

test("private header keys are recognised", () => {
  for (const k of ["Host", "Número de série", "Serial", "Endereço MAC", "Usuário"]) assert.ok(PRIVATE_KEY.test(k), k);
  for (const k of ["Modelo", "Kernel", "Driver"]) assert.ok(!PRIVATE_KEY.test(k), k);
});
