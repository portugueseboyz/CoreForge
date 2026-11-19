import { expect, test } from "vitest";
import { redact, PRIVATE_KEY } from "../src/redact";

test("redact removes personal identifiers but keeps hardware text", () => {
  const raw = "Host: maquina-da-ana\nMAC aa:bb:cc:dd:ee:ff ip 192.168.1.226 fe80:0:0:0:1:2:3:4\nana@maquina-da-ana\n/home/ana/.config /run/user/1000\nRyzen 5 5500 PCI 01:00.0 ligado 02:41:12 4269 MHz";
  const out = redact(raw, ["maquina-da-ana"]);
  for (const leak of ["maquina-da-ana", "aa:bb", "192.168", "fe80", "ana@", "/home/ana", "/1000"]) expect(out).not.toContain(leak);
  for (const keep of ["Ryzen 5 5500", "01:00.0", "02:41:12", "4269 MHz"]) expect(out).toContain(keep);
});

test("private header keys are recognised", () => {
  for (const k of ["Host", "Número de série", "Serial", "Endereço MAC", "Usuário"]) expect(PRIVATE_KEY.test(k)).toBe(true);
  for (const k of ["Modelo", "Kernel", "Driver"]) expect(PRIVATE_KEY.test(k)).toBe(false);
});
