# Status do projeto

- **Data:** 2026-11-12
- **Produto:** D-cut; caminho D. Branch local `master` publica em `main` (`remote.origin.push`).
- **Estado:** Fases 1–7 implementadas, publicadas e verificadas. Corte, texto/legendas na GPU, multi-faixa,
  motor de efeitos (29 efeitos + 16 transições), legenda automática offline (whisper.cpp, modelo escolhido
  pelo usuário), velocidade, keyframes de efeito, áudio por clipe, proxies, autosave/recuperação, i18n
  PT-BR (+ EN parcial), PKGBUILD para o AUR e checagem de atualização opt-in.
- **Entrega:** tag anotada `v0.1.0` → Release pública com AppImage, `.sha256` e `.zsync`. HEAD do `main` é
  `1ba26fe` (seletor de modelo), CI verde. Não existem tags `phase-N`; o histórico por fase está neste arquivo.
- **Divergências aprovadas:** composição de vídeo via grafo FFmpeg (provisória, comparação em DECISIONS);
  texto/legendas compostos no wgpu (`editor-render`); catálogo de modelos de fala em vez de `small` fixo.

## Verificado (comando → resultado)

- `cargo fmt --all --check` ✅ · `cargo clippy --workspace --all-targets -- -D warnings` ✅ ·
  `cargo deny check` ✅ (advisories/bans/licenses/sources ok; exceção justificada RUSTSEC-2026-0192).
- `cargo test --workspace` ✅ **63 testes** (52 das fases 1–6, autosave/recuperação/i18n, 4 do catálogo de
  modelos, 1 de settings e 1 de round-trip do `speech_model`), com **FFmpeg 9.0.2 (host)** e **7.1.5 (embutido)**.
- Playback 1080p30 63,8 s (Vega 8): 0 perdidos, 0 underruns, deriva A/V 3,4 ms. Wayland e X11 fecham com exit 0.
- Seek: 20/20 seeks aleatórios frame-exatos (DECISIONS §47). Comparação FFmpeg-only × wgpu: 79,8 vs 32–38 fps
  no export 1080p; PSNR entre saídas 43,2 dB.
- **Fase 7 — autosave/recuperação:** testes de `editor-project::autosave` (grava, encontra só PIDs mortos,
  recupera e apaga; projeto com mídia em outro diretório volta a apontar para o arquivo absoluto). Em execução
  real com autosave de PID inexistente, o log mostrou `offering crash recovery autosave=… pid=99999999` e a
  saída foi limpa (exit 0).
- **Fase 7 — i18n:** `crates/editor-ui/locales/en.tsv` (150 textos). Teste falha se um `tr("…")` não tiver
  tradução; em execução real `D_CUT_LANG=en` → log `language=en`.
- **Fase 7 — catálogo:** 45 pacotes (29 efeitos + 16 transições), todos com golden test
  (`cargo test -p editor-render --test golden`; cada efeito precisa mudar >100 pixels da entrada, e as
  transições são checadas nos extremos 0/1). Folha de contato dos 30 efeitos em `/tmp/opencode/dcut/grid.png`
  — **revisão visual humana ainda não feita**; os 18 novos entraram só como `manifest.json` + `effect.wgsl`
  (+ golden), sem tocar no motor.
- **Fase 7 — atalhos:** Q/W (cortar início/fim no playhead), ↑/↓ (ponto de edição), F1 (lista) e a janela
  "Atalhos"; desligados quando um campo de texto tem foco (`egui_wants_keyboard_input`).
- **Fase 7 — AUR:** `release/aur/PKGBUILD` compilado por `scripts/test-pkgbuild.sh` (makepkg local, sem sudo),
  pacote de **11.376.910 bytes** com `usr/bin/d-cut`, `usr/bin/d-cut-cli`, `.desktop`, metainfo, ícone,
  `LICENSE` e `THIRD_PARTY_LICENSES`; `sha256sums=('94b3ab76…e4b9')` confere com o tarball de `v0.1.0`.
- **Publicado (v0.1.0):** `github.com/heronkil777-debug/D-cut`, branch única `main`. CI verde em `498b0b9`;
  Release com **`D-cut-x86_64.AppImage` 29.530.616 bytes**, `.sha256` e `.zsync` (101.221 bytes).
  Artefato baixado de novo e conferido: `sha256sum -c` OK, `--cli --version` → `D-cut 0.1.0`,
  `--cli probe` OK **com um `ffmpeg` falso no PATH** (prova o FFmpeg embutido). GUI do artefato publicado:
  com a sessão desbloqueada, `D_CUT_AUTOTEST=quit:4 --appimage-extract-and-run` abre, loga `D-cut exiting` +
  `D-cut window closed cleanly` e sai com **exit 0**. (Com a sessão bloqueada o app espera a janela ser mapeada
  e o teste estoura por timeout — é ambiente, não o artefato.)
- **AppImage (build local):** 29.530.616 bytes, sha256
  `ec32ccba7df4e604a1747795d950b880c40041f7430e2eca885ceb42390fb1da`, `.zsync` de 101.153 bytes, update info
  embutida (`readelf -p .upd_info` → `gh-releases-zsync|heronkil777-debug|D-cut|latest|D-cut-*x86_64.AppImage.zsync`).
  `--cli render-project p720.dcut --fast` com `ffmpeg` falso no PATH → **2100 frames, 70,000 s, 58,3 fps**,
  h264/yuv420p + AAC.
- **Seletor de modelo (`1ba26fe`):** catálogo `MODELS` (`small` 488 MB padrão, `medium` 1,5 GB,
  `large-v3-turbo` 1,6 GB) com URL, tamanho e SHA-256 oficiais; `settings.json` guarda `speech_model` e id
  inválido volta para `small`; a janela de legendas lista os modelos com tamanho/estado e baixa o escolhido;
  CLI ganhou `models`, `download-model --model ID` e `transcribe --model ID`. Verificado: `d-cut-cli models`
  mostra `installed`/`-`, id inválido falha listando os válidos, `transcribe --model large-v3-turbo` usou o
  modelo escolhido (6 s de áudio em 68 s), GUI abre e fecha com exit 0.
- **WER — fala PT-BR:** LibriVox (*O Alienista*), ~2–3% de WER, 1,06× tempo real no 3500U.
- **WER — letra cantada:** vídeo de 2min52s do usuário (58 cues, 374 palavras com o `small`), letra oficial como
  referência: `small` **66,6%** (S=187 D=12 I=34 em 350 palavras, 5min20s) e `large-v3-turbo` **53,7%**
  (S=124 D=29 I=35, 33min36s — estourou a RAM de 5,7 GB e caiu no swap). Nenhum Whisper serve para música.

## Não verificado / não funciona

- Interação real de mouse/teclado na GUI (arrastar, diálogos, edição de texto) — só lógica testada + screenshots.
- Autosave: o disparo periódico de 30 s não foi exercitado na GUI (gravação/recuperação unitária e a janela de
  recuperação em execução real foram).
- i18n parcial: mensagens de status montadas com `format!` (números/nomes) e nomes de efeitos dos manifests
  continuam em pt-BR.
- Checagem de atualização: nunca exercitada de ponta a ponta contra a Release `v0.1.0`.
- AUR: PKGBUILD **não publicado** — criar conta/chave em aur.archlinux.org está falhando para o usuário.
- RX 6650 XT (desktop): nada medido lá; todo desempenho registrado é do Vega 8 (prévia 960×540).
- Prévia converte YUV→RGB com BT.601 (desvio leve de cor só na tela; export preserva os valores YUV).
- Texto: sem fontes do sistema, sem animação/keyframe, sem caixa de fundo; textos sobrepostos na mesma faixa
  ficam desenhados um sobre o outro. Multi-faixa de vídeo é "faixa de cima cobre" (sem PiP/transparência).
- Fase 6: velocidade só constante (sem rampa/curva); transição sem *handles* congela o quadro extremo;
  redução de ruído de áudio não existe.
- Chroma key, máscara e remoção de fundo não existem (`editor-vision` não foi criado).
- Export só MP4 H.264 + AAC; HEVC, VP9/WebM, AV1, GIF e PNG não implementados.
- Aceite de 720p no notebook (~6 GB) nunca foi rodado pelo usuário.
- Tags `phase-1`…`phase-7` não existem; a entrega está na tag `v0.1.0`.
- Modelo de teste `~/.cache/dcut-test/ggml-large-v3-turbo.bin` (1,6 GB) ainda no disco, aguardando decisão.
- Templates e biblioteca de músicas: fora (reavaliado na Fase 7 — ver DECISIONS).

## Próximo passo

1. Usuário rodar o checklist no Arch (notebook e, se possível, desktop RX 6650 XT): editar 720p com a janela
   visível, autosave/recuperação, troca de idioma, atalhos e export.
2. Quando o usuário enviar 1–2 min de fala PT-BR, rodar `d-cut-cli transcribe --model small` e reportar o WER.
3. Publicar o AppImage com o seletor de modelo (`v0.1.1`) e o PKGBUILD no AUR — os dois aguardam autorização
   (o usuário pediu para deixar a Release para quando estiver tudo pronto).
4. Escopo que continua aberto, por demanda: `editor-vision` (chroma key/máscara/remoção de fundo), codecs de
   export além de H.264, rampa de velocidade e redução de ruído.
