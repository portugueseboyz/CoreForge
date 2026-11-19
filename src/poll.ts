// Mesmo nome da função global, mas pausa quando a janela está oculta ou minimizada: nenhuma leitura de sensores em segundo plano.
export const setInterval = (fn: () => void, ms: number): number => window.setInterval(() => { if (!document.hidden) fn(); }, ms);
