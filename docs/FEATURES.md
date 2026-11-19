# Paridade de recursos: ResetExplode (Windows) × CoreForge (Linux)

Legenda: **Feito** = existe no CoreForge; **Novo** = adicionado nesta rodada; **Parcial**; **N/A** = não se aplica ao Linux; **Falta** = faz sentido, ainda não existe.

| Recurso | Original | CoreForge | Equivalente no Linux / motivo |
|---|---|---|---|
| Informações do sistema (SO, placa-mãe, CPU, RAM, GPU, discos) | Sim | Feito (Sistema › Informações) | /sys, /proc, lspci |
| Placa de vídeo (ficha e leituras ao vivo) | Sim | Feito (Sistema › Placa de vídeo) | NVML, sysfs amdgpu/i915 |
| Sensores (temperatura, clock, carga, tensão) | Sim | Feito (Sensores, Histórico) | hwmon |
| Detalhes da CPU (cache, governador, boost, flags) | Parcial | **Novo** (Sistema › CPU) | lscpu, cpufreq, /proc/cpuinfo |
| Verificação de OC / estrangulamento (OcCheck, WHEA) | Sim | **Novo**, parcial (Sistema › Limites) | Throttle térmico, boost, limites RAPL e limitador NVIDIA. Sem equivalente ao WHEA (MCE/EDAC exigem root) |
| Bateria e energia | Sim | Feito (Energia) | /sys/class/power_supply, power-profiles-daemon |
| Stress de CPU/RAM e de GPU | Sim | Feito (Stress) | Implementação própria |
| Benchmarks CPU, GPU e Ray tracing | Sim (D3D12/DXR) | Feito (Vulkan/wgpu) | Notas não são comparáveis com as do Windows |
| Benchmark de disco e saúde (SMART) | Sim | Feito (Disco) | smartmontools (opcional) |
| Teste de rede (latência, DNS, velocidade) | Sim | Feito (Rede) | |
| Velocidade/duplex das interfaces | Sim | Parcial (Rede lista interfaces e velocidade) | Duplex via /sys/class/net, falta |
| Biblioteca de jogos, FPS esperado, atalhos de lançamento | Sim | Feito (Jogos) | Steam/Heroic/Lutris |
| Gargalo (BottleneckAnalyzer) | Sim | Feito (Jogos › Gargalo) | |
| Overlay de FPS personalizável | Sim | Feito (Overlay) | MangoHud |
| Ventoinhas, curvas, perfis | Sim | Feito (Ventoinhas e LEDs) | hwmon pwm via helper com polkit |
| LEDs RGB (Corsair SDK, LightBackends) | Sim | Feito, via OpenRGB | |
| Inicialização (StartupManager) | Sim | Feito (Ferramentas › Inicialização) | Autostart XDG, ajuste só do usuário |
| Instalar apps (winget) | Sim | Feito (Instalar apps) | Flatpak/apt |
| Portas COM e drivers seriais (RESET MAKER) | Sim | Feito, adaptado (Portas e USB) | dialout, brltty, udev; drivers já vêm no kernel |
| Limpeza / Otimização (Optimizer) | Sim | **Novo** (Ferramentas › Limpeza) | Miniaturas, lixeira e ~/.cache limpáveis (um por vez, com confirmação); cache de pacotes, journal e kernels antigos só listados com o comando (exigem root) |
| Limpeza de USB fantasma (UsbCleaner) | Sim | N/A | Não existe registro de dispositivos fantasmas; o udev os remove sozinho |
| Gerenciador de drivers (DriverManager, Drivers) | Sim | N/A | Drivers vêm do kernel/distribuição; GPU proprietária via o gerenciador da distro |
| Tweaks do Windows (WindowsTweaks) | Sim | N/A | Específico do Windows |
| Informações da memória (tipo, velocidade, slots) | Sim (WMI) | Falta | Exige dmidecode (root): fica para uma ação nova do helper com polkit |
| Benchmark de memória (MemoryBench) | Sim | Parcial | Coberto pelo stress de RAM |
| Relatório (ReportBuilder) | Sim | Feito (Salvar relatório) | |
| Idioma, bandeja, alerta de temperatura | Sim | Feito (Configurações) | pt/en |
| Instalador/atualização do app (AppInstaller) | Sim | N/A | Pacotes deb/rpm |
