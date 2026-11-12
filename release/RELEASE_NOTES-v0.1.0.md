# D-cut 0.1.0

Editor de vídeo local-first para Linux: corte, texto e legendas na GPU, efeitos, transições e
exportação H.264/AAC — sem conta, sem nuvem, sem telemetria.

## Destaques

- **Timeline multi-faixa** com corte, trim, ripple, snap, zoom, arrastar-e-soltar e undo/redo.
- **Texto e legendas compostos na GPU** (mesmo shader na prévia e no export), com contorno e
  posicionamento por projeto (16:9, 9:16, 1:1, 4:5).
- **Legendas automáticas offline em PT-BR** via `whisper.cpp` (modelo baixado só quando você pede,
  verificado por SHA-256, 487 MB, MIT).
- **45 pacotes de efeito** (29 efeitos + 16 transições), cada um com teste de imagem de referência.
- **Keyframes de efeito** (hold/linear/smooth), velocidade 0,25–4×, ganho/fade/redução de ruído por clipe.
- **Proxies 540p** gerados em segundo plano para editar 720p/1080p em notebook modesto; o export usa
  sempre os arquivos originais.
- **Autosave a cada 30 s** com recuperação após queda, e interface em **PT-BR e EN**.
- **AppImage portátil** (~29 MB) com FFmpeg 7.1.5 (GPL, x264/x265/SVT-AV1/VAAPI) embutido.

## Instalação

### AppImage (qualquer distro com glibc ≥ 2.35)

```bash
chmod +x D-cut-x86_64.AppImage
./D-cut-x86_64.AppImage
```

Precisa de FUSE 2. Sem FUSE: `./D-cut-x86_64.AppImage --appimage-extract-and-run`.
Atualização delta opcional por AppImageUpdate/Gear Lever (o app só verifica versão se você ligar
"Verificar atualizações" nos ajustes).

### Arch Linux (AUR, compila do fonte)

```bash
yay -S d-cut        # ou: paru -S d-cut
```

Requisitos: FFmpeg, ALSA, Vulkan (driver da sua GPU: `vulkan-radeon`, `vulkan-intel`, …).
Opcional: `whisper.cpp` para legendas automáticas e `libva-mesa-driver` para export por VAAPI.

## Requisitos

- Linux x86-64, glibc ≥ 2.35 (AppImage).
- GPU com Vulkan (a prévia usa wgpu; sem Vulkan a prévia não inicia).
- CPU com AVX2 para as legendas automáticas (whisper.cpp).

## Limitações conhecidas

- Prévia limitada a 960×540; o export usa a resolução do projeto.
- Transição sem quadros de sobra congela o quadro extremo (sem *handles*).
- Faixa de vídeo de cima cobre a de baixo: sem PiP nem transparência entre faixas.
- Textos na mesma faixa desenham um sobre o outro.
- Velocidade só constante (sem rampa).
- Tradução EN parcial: mensagens com números/nomes e nomes de efeitos continuam em PT-BR.
- A checagem de atualização só avisa; não baixa nem instala.
- Sem templates nem biblioteca de músicas (decisão registrada em `docs/DECISIONS.md`).

## Verificação

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo deny check` e `cargo test --workspace` (55 testes) verdes.
- AppImage testado com um `ffmpeg` falso no `PATH` (prova que usa o embutido): export de 2100 frames
  em 70 s, H.264/yuv420p + AAC.
- Comparação medida entre composição FFmpeg e GPU registrada em `docs/DECISIONS.md`.

## Licença

AGPL-3.0-or-later. FFmpeg e whisper.cpp vão com suas licenças em `usr/share/licenses/`.
