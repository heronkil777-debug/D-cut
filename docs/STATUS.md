# Status do projeto

- **Data:** 2026-11-11
- **Produto:** D-cut; caminho D.
- **Estado honesto:** editor de corte **utilizável numa timeline simples** (V1 + A1 + A2): importar,
  prévia com frames reais, play com áudio, dividir/aparar/mover/excluir/ripple com undo/redo,
  salvar/abrir, exportar MP4 H.264/AAC. **Nenhuma fase do briefing está 100% aceita** (faltam CI
  verificado, AppImage em container, FFmpeg embutido, texto, efeitos etc.). Nenhuma tag `phase-N`.

## Verificado nesta sessão (comando → resultado)

- `cargo build --workspace` → ok. `cargo fmt --all --check` → ok.
  `cargo clippy --workspace --all-targets -- -D warnings` → ok. `cargo test --workspace` → 28 testes ok:
  - core: proptest de sequências aleatórias (add/split/delete/ripple/trim/move) com undo de cada passo e redo total; clipes vinculados; limites de trim; validação de sobreposição; plano de render.
  - project: roundtrip JSON, caminhos relativos, versão inválida.
  - media (FFmpeg real 9.0.2): export de timeline editada validado com ffprobe (h264 1920×1080 yuv420p 30/1, 60 frames exatos, AAC 48 kHz, duração 2,0 s, moov antes de mdat, frame exportado = frame-fonte correto); 20 seeks aleatórios frame-exatos; clipe com rotação 90° (720×1280, preenche canvas 9:16) e clipe VFR (30→10 fps) exportados com contagem de frames exata.
- `scripts/test-media-workflow.sh` → ok.
- GUI no Wayland (MangoWC, Vega 8/RADV), por screenshot (`grim`): prévia mostra frame real com rotação aplicada; thumbnails na faixa de vídeo; waveform no áudio; layout corrigido para janela estreita.
- Playback medido (`D_CUT_AUTOTEST`, log): 1080p30, 63,8 s com relógio de áudio → 1915 frames, 0 perdidos, 0 underruns, **deriva 3,4 ms**, |offset| máx 16,9 ms. Fecha com exit 0 em Wayland e em X11 (Xwayland, `DISPLAY=:1`).
- AppImage (`scripts/build-appimage.sh --host`) → `build/appimage/D-cut-x86_64.AppImage`, ~8,4 MB:
  `--cli --version` via FUSE ok; `--appimage-extract-and-run --cli render-project` → 120 frames, 4,000 s, h264 High yuv420p, AAC 48 kHz estéreo, faststart; perfil `--fast` (VAAPI) → 120 frames ok; GUI do AppImage abriu o projeto, tocou 4 s (120 frames, 0 perdidos) e fechou com exit 0.

## Não verificado / não funciona

- Interação real com mouse/teclado na GUI (arrastar trim/mover, diálogos, atalhos, salvar pela UI) **não foi exercitada por mim** — só a lógica (testes) e o render (screenshot). Ver checklist no relatório.
- Container Ubuntu 22.04: aguardando o usuário instalar Podman (`sudo pacman -S podman`); script pronto (`--userns=keep-id`). AppImage atual (host) exige glibc ≥ 2.44 e **não é final** sem FFmpeg 7+ embutido.
- FFmpeg não está embutido no AppImage (usa o do sistema). `cargo deny` não instalado → não rodado. CI do GitHub não verificado.
- Latência do dispositivo de áudio não compensada. Sem autosave. i18n só parcial (catálogo pt-BR em `strings.rs`).
- Fora do feito: texto/legendas no render, efeitos/transições GPU, keyframes/velocidade, multi-faixa de vídeo com overlay (só prioridade da faixa), legenda automática, proxies, HDR→SDR com aviso.
- Composição feita no FFmpeg, não no wgpu (ver DECISIONS) — decisão pendente do usuário antes da Fase 3.

## Próximo passo

1. Usuário testar o checklist manual no Arch e decidir backend FFmpeg/composição.
2. Habilitar Podman (ou grupo docker) para gerar o AppImage portátil; decidir FFmpeg embutido.
3. Fase 3: texto/títulos/SRT no caminho de render compartilhado.
