# Status do projeto

- **Data:** 2026-11-11
- **Produto:** D-cut; caminho D. Branch local `master` (repo recriado do ZIP).
- **Estado:** editor de corte utilizável + Fase 3 (texto/legendas na GPU, multi-faixa). Fases 1–3 com
  critérios atendidos localmente; **sem tags `phase-N`** porque o CI do GitHub nunca rodou (sem push).
- **Divergências aprovadas:** composição de vídeo via grafo FFmpeg (provisória, comparação em DECISIONS);
  texto/legendas compostos no wgpu (`editor-render`) como pede o briefing.

## Verificado (comando → resultado)

- `cargo fmt --all --check` ✅ · `cargo clippy --workspace --all-targets -- -D warnings` ✅ ·
  `cargo deny check` ✅ (advisories/bans/licenses/sources ok; exceção justificada RUSTSEC-2026-0192).
- `cargo test --workspace` ✅ com **FFmpeg 9.0.2 (host)** e com **FFmpeg 7.1.5 (embutido)**: core (proptest de edição,
  faixas, texto), project, media (export validado por ffprobe, 20 seeks frame-exatos, rotação/VFR, stream RGBA com
  contagem exata), text (centralização, contorno, escala, tempo), render (composição GPU headless), export
  (título + SRT queimados em 9:16 e 1:1 com posição/tempo checados; caminho GPU ≥ 38 dB vs FFmpeg-only).
- Playback 1080p30 63,8 s (Vega 8): 0 perdidos, 0 underruns, deriva A/V 3,4 ms. Wayland e X11 fecham com exit 0.
- GUI (screenshot): prévia com título e legenda compostos na GPU sobre o frame real (9:16).
- Comparação FFmpeg-only × wgpu medida (DECISIONS): 79,8 vs 32–38 fps no export 1080p; PSNR entre saídas 43,2 dB.
- AppImage portátil (container Ubuntu 22.04, Podman rootless `--network=host`): ~27 MB, glibc ≥ 2.35,
  FFmpeg 7.1.5 GPL embutido (x264, x265, SVT-AV1, VAAPI; sem nonfree), licenças + manifesto em
  `usr/share/licenses/ffmpeg/`. Testes do artefato no relatório da sessão.

## Não verificado / não funciona

- Interação real de mouse/teclado na GUI (arrastar, diálogos, edição de texto) — só lógica testada + screenshots.
- RX 6650 XT (desktop): nada medido lá ainda. CI do GitHub não rodado.
- Prévia converte YUV→RGB com BT.601 (desvio leve de cor só na tela; export preserva os valores YUV).
- Texto: sem fontes do sistema, sem animação/keyframe, sem caixa de fundo; textos sobrepostos na mesma faixa ficam
  desenhados um sobre o outro na timeline. Multi-faixa de vídeo é "faixa de cima cobre" (sem PiP/transparência).
- Fases 4–7 não iniciadas (efeitos, legenda automática, keyframes, transições, proxies, autosave, i18n completo).

## Próximo passo

1. Usuário rodar o checklist do relatório no Arch (desktop RX 6650 XT e notebook).
2. YUV↔RGB no shader (remove o tráfego RGBA e o desvio de cor) e repetir a comparação FFmpeg × wgpu.
3. Fase 4 (motor de efeitos) após "ok".
