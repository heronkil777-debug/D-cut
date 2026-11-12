# Status do projeto

- **Data:** 2026-11-11
- **Produto:** D-cut; caminho D. Branch local `master` (repo recriado do ZIP).
- **Estado:** Fases 1–7 implementadas: corte, texto/legendas na GPU, multi-faixa, motor de efeitos (29 efeitos +
  16 transições), legenda automática offline (whisper.cpp), velocidade, keyframes de efeito, áudio por clipe,
  proxies, autosave/recuperação, i18n PT-BR/EN, PKGBUILD para o AUR e checagem de atualização opt-in.
  **Sem tags `phase-N`** porque o CI do GitHub nunca rodou (sem push).
- **Divergências aprovadas:** composição de vídeo via grafo FFmpeg (provisória, comparação em DECISIONS);
  texto/legendas compostos no wgpu (`editor-render`) como pede o briefing.

## Verificado (comando → resultado)

- `cargo fmt --all --check` ✅ · `cargo clippy --workspace --all-targets -- -D warnings` ✅ ·
  `cargo deny check` ✅ (advisories/bans/licenses/sources ok; exceção justificada RUSTSEC-2026-0192).
- `cargo test --workspace` ✅ **55 testes** (52 das fases 1–6 + autosave, recuperação com mídia em outro diretório e i18n), com **FFmpeg 9.0.2 (host)** e
  **7.1.5 (embutido)**.
- Playback 1080p30 63,8 s (Vega 8): 0 perdidos, 0 underruns, deriva A/V 3,4 ms. Wayland e X11 fecham com exit 0.
- Comparação FFmpeg-only × wgpu medida (DECISIONS): 79,8 vs 32–38 fps no export 1080p; PSNR entre saídas 43,2 dB.
- **Fase 7 — autosave/recuperação:** testes unitários de `editor-project::autosave` (grava, encontra só PIDs mortos,
  recupera e apaga; e um projeto com mídia recuperado de outro diretório volta a apontar para o mesmo arquivo absoluto). Em execução real, com um autosave de PID inexistente:
  `D_CUT_LANG=en D_CUT_AUTOSAVE_DIR=… ./target/debug/d-cut` → log
  `offering crash recovery autosave=…/autosave-99999999.dcut original=Some("/home/hero/Videos/meu-video.dcut") pid=99999999`;
  saída limpa com exit 0.
- **Fase 7 — i18n:** catálogo `crates/editor-ui/locales/en.tsv` (148 textos). Teste falha se um `tr("…")` não tiver
  tradução; em execução real `D_CUT_LANG=en` → log `D-cut starting version="0.1.0" language=en`.
- **Fase 7 — catálogo:** 45 pacotes (29 efeitos + 16 transições), todos com golden test
  (`cargo test -p editor-render --test golden`; cada efeito precisa mudar >100 pixels da imagem de entrada, e as
  transições também são checadas nos extremos 0/1). Folha de contato dos 30 efeitos gerada em
  `/tmp/opencode/dcut/grid.png` — **revisão visual ainda não feita** (o agente desta sessão não lê imagens); os 18
  novos entraram só como `manifest.json` + `effect.wgsl` (+ golden), sem tocar no motor.
- **Fase 7 — atalhos:** Q/W (cortar início/fim no playhead), ↑/↓ (ponto de edição), F1 (lista), e a lista completa
  na janela "Atalhos"; atalhos desligados quando um campo de texto tem foco (`egui_wants_keyboard_input`).
- **Fase 7 — AUR:** `release/aur/PKGBUILD` compilado por `scripts/test-pkgbuild.sh` (makepkg local, sem sudo,
  `--nodeps` porque o cargo vem do rustup): pacote **11.376.910 bytes** com `usr/bin/d-cut`, `usr/bin/d-cut-cli`,
  `.desktop`, metainfo, ícone, `LICENSE` e `THIRD_PARTY_LICENSES`.
- **Publicado no GitHub (v0.1.0):** repositório `github.com/heronkil777-debug/D-cut`, branch único `main`
  (`master` local publica nele via `remote.origin.push`). CI verde em `498b0b9` (run 43141603483 e 43143616800);
  tag anotada `v0.1.0` → **Release com `D-cut-x86_64.AppImage` 29.530.616 bytes, `.sha256` e `.zsync`** (101.221 bytes).
  Artefato publicado baixado de novo e conferido: `sha256sum -c` OK, `--cli --version` → `D-cut 0.1.0` (exit 0),
  `--cli probe` OK **com um `ffmpeg` falso no PATH** (prova que usa o embutido).
  GUI do artefato publicado: com a sessão desbloqueada, `D_CUT_AUTOTEST=quit:4 --appimage-extract-and-run` abre,
  loga `D-cut exiting` + `D-cut window closed cleanly` e sai com **exit 0**. (Com a sessão bloqueada o app fica
  esperando a janela ser mapeada e o teste estoura por timeout — é ambiente, não o artefato.)
- **AUR:** `release/aur/PKGBUILD` com `sha256sums=('94b3ab76…e4b9')` = checksum real do tarball de `v0.1.0`
  (baixado e conferido). `.SRCINFO` regenerado. **Envio pendente:** exige chave SSH cadastrada em aur.archlinux.org.
- **Publicação (kit):** `release/` tem o roteiro (`README.md`), release notes, `PKGBUILD` + `.SRCINFO` canônicos,
  `set-repo.sh`, `fill-sha256sums.sh` e `prepare-aur.sh`. O CI embute a update info a partir de `github.repository` e,
  na tag, cria o GitHub Release com AppImage + `.sha256` + `.zsync`.
- **Fase 7 — AppImage:** `build/appimage/D-cut-x86_64.AppImage` **29.530.616 bytes**, sha256
  `ec32ccba7df4e604a1747795d950b880c40041f7430e2eca885ceb42390fb1da`, `.zsync` de 101.153 bytes,
  update info embutida (`readelf -p .upd_info` → `gh-releases-zsync|heronkil777-debug|D-cut|latest|D-cut-*x86_64.AppImage.zsync`).
  Execução: `--appimage-extract-and-run` inicia e fecha com exit 0 (log `language=en`); com um `ffmpeg` falso no PATH
  (prova que usa o embutido) `--cli render-project p720.dcut --fast` → **2100 frames, 70,000 s, 58,3 fps**, h264/yuv420p + AAC.

## Não verificado / não funciona

- Interação real de mouse/teclado na GUI (arrastar, diálogos, edição de texto) — só lógica testada + screenshots.
- Autosave: o disparo periódico de 30 s não foi exercitado na GUI (só a gravação/recuperação unitária e a janela
  de recuperação em execução real); a gravação em si é atômica e testada.
- i18n parcial: mensagens de status montadas com `format!` (números/nomes) e nomes de efeitos dos manifests
  continuam em pt-BR. O catálogo de chaves já está centralizado, então trocar de mecanismo é mecânico.
- Checagem de atualização: nunca exercitada contra um release real (o repositório ainda não tem releases).
- AUR: PKGBUILD **não publicado**; `sha256sums` é `SKIP` até a primeira tag. Nada foi enviado ao AUR.
- RX 6650 XT (desktop): nada medido lá ainda. CI do GitHub não rodado.
- Prévia converte YUV→RGB com BT.601 (desvio leve de cor só na tela; export preserva os valores YUV).
- Texto: sem fontes do sistema, sem animação/keyframe, sem caixa de fundo; textos sobrepostos na mesma faixa ficam
  desenhados um sobre o outro na timeline. Multi-faixa de vídeo é "faixa de cima cobre" (sem PiP/transparência).
- Fase 4: desempenho medido só no Vega 8 (prévia 960×540); falta RX 6650 XT e CI com lavapipe.
- Fase 5: qualidade avaliada com LibriVox (O Alienista, ~2–3% WER, 1,06× tempo real no 3500U); falta o áudio do usuário.
- Fase 6: só velocidade constante (sem rampa); transição sem *handles* congela o quadro extremo; prévia limitada a 960×540.
- Templates e biblioteca de músicas: continuam fora (reavaliados na Fase 7 — ver DECISIONS).

## Próximo passo

1. Usuário rodar o checklist no Arch (notebook e, se possível, desktop RX 6650 XT): editar 720p com a janela
   visível, autosave/recuperação, troca de idioma, atalhos novos e export.
2. Quando o usuário enviar 1–2 min de áudio PT-BR, rodar `d-cut-cli transcribe` e reportar o WER real.
3. Antes de publicar: rodar o CI no GitHub (push), criar a tag `v0.1.0`, preencher `sha256sums` do PKGBUILD e
   enviar ao AUR.

- **Seletor de modelo (v0.1.1):** `editor-speech` tem catálogo `MODELS` (small/medium/large-v3-turbo com
  SHA-256 e tamanho oficiais), `settings.json` guarda `speech_model` (id inválido → `small`), a janela de
  legendas lista os modelos com tamanho/estado e baixa o escolhido, e a CLI ganhou `models`,
  `download-model --model ID` e `transcribe --model ID`. Verificado: `d-cut-cli models` lista installed/-,
  id inválido falha listando os válidos, e `transcribe --model large-v3-turbo` usou o modelo escolhido
  (6 s de áudio em 68 s). Testes: 62 (4 do catálogo + 1 de settings).
- **WER medido em letra cantada:** `small` 66,6% e `large-v3-turbo` 53,7% (vídeo de 2min52s, letra oficial
  como referência). Nenhum dos dois serve para música; o número de **fala** em PT-BR continua pendente.
