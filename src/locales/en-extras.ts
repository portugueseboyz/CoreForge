// Traduções das páginas Limpeza, CPU e Limites (texto do Rust incluso).
export const EXTRA_EXACT: Record<string, string> = {
  "Detalhes do processador: cache, frequência por núcleo, governador e instruções.": "Processor details: cache, per-core frequency, governor and instruction sets.",
  "Verifica se a CPU ou a GPU estão sendo limitadas por calor ou potência.": "Checks whether the CPU or GPU is being limited by heat or power.",
  "Veja o que ocupa espaço e limpe uma categoria por vez.": "See what takes up space and clean one category at a time.",
  "Mostra o que ocupa espaço. Só as categorias do seu usuário podem ser limpas aqui, uma por vez e com confirmação; o que exige administrador aparece com o comando para rodar no terminal.": "Shows what takes up space. Only your own user's categories can be cleaned here, one at a time and with confirmation; anything that needs admin shows the command to run in a terminal.",
  "Calculando tamanhos…": "Calculating sizes…", "Não foi possível ler as categorias.": "Could not read the categories.",
  "Limpando…": "Cleaning…", "Limpar agora": "Clean now", "Recalcular": "Recalculate", "Limpeza": "Cleanup",
  "Miniaturas de imagens": "Image thumbnails", "Lixeira": "Trash", "Cache dos programas (~/.cache)": "Program cache (~/.cache)",
  "Os programas recriam o que precisarem; o primeiro uso depois pode ser mais lento.": "Programs recreate what they need; the first use afterwards may be slower.",
  "Cache de pacotes do sistema": "System package cache", "Logs do sistema (journal)": "System logs (journal)", "Kernels antigos": "Old kernels",
  "Nenhum kernel antigo encontrado (ou o sistema não usa dpkg).": "No old kernels found (or the system does not use dpkg).",
  "Lendo a CPU…": "Reading the CPU…", "O comando lscpu não está disponível.": "The lscpu command is not available.",
  "Frequência": "Frequency", "Governador": "Governor", "Driver": "Driver", "Boost / turbo": "Boost / turbo", "Ligado": "On", "Desligado": "Off",
  "O kernel não expõe a frequência (cpufreq) neste PC.": "The kernel does not expose frequency (cpufreq) on this PC.",
  "Instruções": "Instructions", "Sem informações de instruções.": "No instruction information.",
  "Verificando…": "Checking…", "Verificar de novo": "Check again", "Nenhum": "None", "Limitador da GPU (NVIDIA)": "GPU limiter (NVIDIA)",
  "Leitura somente: mostra se o kernel está limitando a CPU ou a GPU por temperatura ou potência. Nada é alterado.": "Read-only: shows whether the kernel is limiting the CPU or GPU because of temperature or power. Nothing is changed.",
  "Não informado pelo kernel": "Not reported by the kernel", "Núcleos com clock máximo limitado": "Cores with capped max clock",
  "Estrangulamentos térmicos desde o boot": "Thermal throttle events since boot", "Limites": "Limits",
};
export const EXTRA_TEMPLATES: [string, string][] = [
  ["Apagar o conteúdo de “{0}” ({1})? Isso não pode ser desfeito.", "Delete the contents of “{0}” ({1})? This cannot be undone."],
  ["{0} liberados.", "{0} freed."],
  ["Exige root; rode no terminal: {0}", "Needs root; run in a terminal: {0}"],
  ["{0} (núcleo) · {1} (pacote)", "{0} (core) · {1} (package)"],
  ["Limite de potência {0} ({1})", "Power limit {0} ({1})"],
];
