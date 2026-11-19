[English](README.md) | Português

# CoreForge

> Versão inicial (0.1). Testado em um único desktop (GPU NVIDIA, controlador de ventoinhas Nuvoton nct6798). Controle de ventoinhas, notebooks, GPUs AMD/Intel e outras lojas de jogos estão implementados, mas ainda não foram verificados em outros equipamentos. Ainda não há instalador: execute a partir do código-fonte.

Kit de ferramentas de hardware para Linux: informações do sistema, sensores ao vivo, testes de estresse de CPU/RAM/GPU, benchmarks de CPU/GPU/ray tracing, diagnóstico de disco e rede, controle de velocidade das ventoinhas e iluminação RGB. Feito com [Tauri 2](https://tauri.app) (backend em Rust, interface em React + TypeScript).

Idiomas da interface: inglês e português (troque em Configurações). O app é um port para Linux inspirado em
[GustavoReset/ResetExplode](https://github.com/GustavoReset/ResetExplode) (MIT, Windows/WPF); veja `LICENSE`.

## O que ele encontra sozinho

- **Ventoinhas:** varre todos os chips `hwmon` e a GPU NVIDIA, tenta o driver de placa-mãe correto para o seu fabricante (`nct6775`, `it87`, …) e executa um teste que associa cada controle PWM à ventoinha que ele realmente move.
- **RGB:** detecta hardware RGB USB e I²C e o modelo da placa de vídeo, informa o que o OpenRGB consegue controlar e cria regras udev apenas para os dispositivos encontrados.
- **Página de preparação:** verifica sensores de CPU/disco, Vulkan, SMART, o helper privilegiado e as ferramentas do sistema, e oferece uma correção para cada item ausente.

## Outros recursos

- **Perfis:** salve as velocidades de ventoinha e as configurações de LED que o app aplicou, reaplique com um clique e, opcionalmente, reaplique as luzes ao iniciar a sessão
  (`coreforge --apply-profile`, sem janela e sem senha; as ventoinhas exigem a senha do sistema, então só são aplicadas pelo app).
- **Curvas de ventoinha:** curvas temperatura→velocidade por ventoinha (fonte CPU ou GPU, 2–8 pontos, predefinições) em Ventoinhas e LEDs → Curvas. Um motor privilegiado
  (`coreforge-helper curve-run`, uma senha) as aplica enquanto o app estiver aberto, sobe rápido e desce devagar, vai a 100 % se a
  temperatura não puder ser lida e devolve todas as ventoinhas ao automático ao ser parado ou quando o app fecha.
- **Alerta de temperatura:** avisa na tela e com uma notificação do desktop (`notify-send`) quando a CPU ou a GPU ficam acima de um limite (padrão 90 °C,
  alterável em Sensores). "Manter na bandeja ao fechar" (também em Sensores) mantém os alertas ativos com a janela fechada; vem desativado por padrão porque
  alguns desktops não mostram bandeja.
- **Notebooks:** nível, saúde, ciclos e consumo da bateria, além do perfil de energia (`power-profiles-daemon`), em Sistema → Energia.
- **GPUs:** NVIDIA via NVML; AMD e Intel via sysfs (Intel não mostra uso nem VRAM).

## Privilégios

A leitura não exige privilégios. Tudo que escreve no hardware (velocidade de ventoinha, carregar drivers, instalar pacote, regras udev) passa pelo
`coreforge-helper`, um binário separado instalado em `/usr/libexec/coreforge/` e executado com `pkexec` (política polkit em
`src-tauri/packaging/org.coreforge.helper.policy`). Ventoinhas e curvas pedem a senha uma vez (lembrada por alguns minutos); instalar pacotes, regras udev, drivers de todo boot e acesso às portas seriais pedem a senha toda vez. Ele aceita um conjunto fechado de ações com argumentos validados e nunca executa texto livre.
Em desenvolvimento, usa o helper compilado junto ao app (`cargo build --bin coreforge-helper` roda automaticamente com `npm run tauri dev`).

## Instalação

Baixe na página de [Releases](https://github.com/portugueseboyz/CoreForge/releases) (quando existir uma) ou compile você mesmo (`npx tauri build`, veja Desenvolvimento).

- **`.deb` (Debian, Ubuntu, Mint):** `sudo apt install ./CoreForge_*_amd64.deb`. Instala o app, o ajudante com privilégios em `/usr/libexec/coreforge/`
  e a política do polkit, então controle de ventoinhas, curvas e instalação de drivers funcionam.
- **`.rpm` (Fedora, openSUSE):** `sudo dnf install ./CoreForge-*.x86_64.rpm`.
- **AppImage:** roda em qualquer lugar, mas não consegue instalar o ajudante com privilégios, então o que pede senha (ventoinhas, curvas, instalar
  pacotes) fica indisponível. Leitura de sensores, testes de stress, benchmarks e relatórios funcionam.

## Desenvolvimento

Requisitos: Node 20+, Rust stable e as dependências Linux do Tauri (`libwebkit2gtk-4.1-dev`, `build-essential`, `libssl-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`).

```bash
npm install
npm run tauri dev        # app com hot reload
npm run dev              # só a interface; abra http://localhost:1420/?mock para usar um backend falso
(cd src-tauri && cargo test)
```

Os testes de GPU precisam de Vulkan; ray tracing precisa de `VK_KHR_ray_query`. Ferramentas opcionais: `smartmontools` (saúde do disco), OpenRGB (LEDs).

## Avisos de segurança

Controle de ventoinhas, carregamento de módulos do kernel e acesso I²C podem afetar o hardware. O app pede confirmação e a senha do sistema a cada
alteração, mantém um piso de 25 % na velocidade manual das ventoinhas e pode devolver ao automático todas as ventoinhas que alterou. Use por sua conta e risco (MIT, sem garantia).

## Privacidade

O CoreForge não tem telemetria. Além dos testes de velocidade que você mesmo inicia, o único uso de rede é a verificação de atualizações: uma requisição à API pública do GitHub (`api.github.com`, última versão) ao abrir e no máximo uma vez por dia, enviando apenas `User-Agent: CoreForge/<versão>`. Desligue em Configurações > "Verificar atualizações".

## Feedback

Encontrou um erro, tem uma ideia ou quer contar como roda no seu hardware? Use os [formulários de issue](https://github.com/portugueseboyz/CoreForge/issues/new/choose)
ou abra uma [discussão](https://github.com/portugueseboyz/CoreForge/discussions). Em português ou inglês. Só o mantenedor altera este repositório;
sugestões chegam como issues, discussões ou pull requests, que são revisados antes de qualquer merge.
