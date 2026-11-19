// Limpeza do relatório: só hardware sai do app. Sem nome de usuário, hostname, número de série, MAC ou IP.
export const PRIVATE_KEY = /host|serial|n[uú]mero de s[eé]rie|uuid|\bmac\b|endere[cç]o|usu[aá]rio|\buser\b/i;
const MARK = "[removido]";
const PATTERNS: RegExp[] = [
  /\b(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b/gi, // MAC
  /\b(?:[0-9a-f]{1,4}:){4,7}[0-9a-f]{1,4}\b/gi, // IPv6 (3 grupos ou menos ficam: horas e slots PCI)
  /\b(?:\d{1,3}\.){3}\d{1,3}\b/g, // IPv4
  /\b[\w.-]+@[\w.-]+\.?\w*\b/g, // usuário@host, e-mail
];
/** `secrets`: valores conhecidos (hostname etc.) removidos onde aparecerem, além dos padrões acima. */
export function redact(text: string, secrets: string[] = []): string {
  let out = text.replace(/\/(home|Users)\/[^/\s)]+/g, "/$1/~").replace(/\/run\/user\/\d+/g, "/run/user/~");
  for (const re of PATTERNS) out = out.replace(re, MARK);
  for (const s of secrets) if (s.length >= 3) out = out.split(s).join(MARK);
  return out;
}
