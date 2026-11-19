// Lógica pura da UI (sem React), extraída do App para poder ser testada.

/** Limite de temperatura (°C) para o alerta; 0 desliga. Lê do armazenamento dado (padrão 90). */
export const tempLimit = (store: Pick<Storage, "getItem"> = localStorage) => { const v = store.getItem("tempLimit"); return v == null ? 90 : Number(v) || 0; };

/** Coordenadas do gráfico de prévia da curva (viewBox 220x110; eixo de 20 a 110 °C, 0 a 100 %). */
export const curveX = (t: number) => 8 + ((t - 20) / 90) * 204;
export const curveY = (p: number) => 102 - (p / 100) * 94;

/** Pontos da curva estendidos até as bordas (20 e 110 °C) com o valor do primeiro e do último ponto. */
export const extendCurve = (points: [number, number][]): [number, number][] =>
  [[20, points[0]?.[1] ?? 0], ...points, [110, points[points.length - 1]?.[1] ?? 0]];
