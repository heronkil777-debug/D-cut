# D-cut 0.1.1

Atualização pequena e focada: agora **você escolhe o modelo de transcrição**, e a documentação passa a
refletir o estado real do projeto. Nada de conta, nuvem ou telemetria.

## O que mudou

- **Seletor de modelo de legenda automática.** O modelo deixou de ser fixo no `small`. A janela
  *Legendas automáticas* lista `small` (488 MB), `medium` (1,5 GB) e `large-v3-turbo` (1,6 GB), com
  tamanho, estado de instalação e botão para baixar o escolhido — download opt-in, conferido por SHA-256
  oficial do `ggerganov/whisper.cpp`, fora do AppImage.
- **A escolha fica salva** em `settings.json` (`speech_model`) e vale para a próxima abertura; valor
  desconhecido volta para `small` sem quebrar nada.
- **CLI acompanhou:** `d-cut-cli models`, `download-model --model ID` e `transcribe --model ID`.
  Id inválido falha listando os válidos, em vez de cair silenciosamente no padrão.
- **Números honestos de qualidade** (registrados em `docs/DECISIONS.md`): fala em PT-BR ficou em
  **2–3% de WER** (LibriVox, *O Alienista*); letra cantada ficou em **66,6%** (`small`) e **53,7%**
  (`large-v3-turbo`) — nenhum Whisper transcreve música de forma utilizável, e o app não promete isso.
- **Documentação reconciliada** com o código: `docs/BRIEFING.md` marca cada critério de fase como
  cumprido, parcial ou ausente, e `docs/STATUS.md` virou um retrato único do que foi verificado.

## Instalação

### AppImage (qualquer distro com glibc ≥ 2.35)

```bash
chmod +x D-cut-x86_64.AppImage
./D-cut-x86_64.AppImage
```

Precisa de FUSE 2. Sem FUSE: `./D-cut-x86_64.AppImage --appimage-extract-and-run`.
Atualização delta opcional por AppImageUpdate/Gear Lever (o app só verifica versão se você ligar
"Verificar atualizações" nos ajustes).

### Arch Linux (AUR)

O PKGBUILD está pronto em `release/aur/`, mas **ainda não foi enviado ao AUR**. Enquanto isso, compile do
fonte: `git clone` + `cargo build --release -p editor-ui`.

Requisitos: FFmpeg, ALSA, Vulkan (driver da sua GPU: `vulkan-radeon`, `vulkan-intel`, …).
Opcional: `libva-mesa-driver` para export por VAAPI. O AppImage já traz `whisper-cli` embutido.

## Requisitos

- Linux x86-64, glibc ≥ 2.35 (AppImage).
- GPU com Vulkan (a prévia usa wgpu; sem Vulkan a prévia não inicia).
- CPU com AVX2 para as legendas automáticas.
- Para `large-v3-turbo`: bastante RAM livre (1,6 GB de modelo; com 6 GB no total ele cai no swap e fica
  ~6× mais lento que o `small`).

## Limitações conhecidas

- Prévia limitada a 960×540; o export usa a resolução do projeto.
- Transição sem quadros de sobra congela o quadro extremo (sem *handles*).
- Faixa de vídeo de cima cobre a de baixo: sem PiP nem transparência entre faixas.
- Textos na mesma faixa desenham um sobre o outro.
- Velocidade só constante (sem rampa); keyframes existem para efeitos.
- Tradução EN parcial: mensagens com números/nomes e nomes de efeitos continuam em PT-BR.
- Export só em MP4 H.264 + AAC; HEVC, VP9/WebM, AV1, GIF e PNG ainda não.
- Sem chroma key, máscara ou remoção de fundo.
- A checagem de atualização só avisa; não baixa nem instala.
- Sem templates nem biblioteca de músicas (decisão registrada em `docs/DECISIONS.md`).

## Verificação

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo deny check` e `cargo test --workspace` (**63 testes**) verdes.
- Seletor exercitado na CLI: `models` lista `installed`/`-`, id inválido falha com a lista de válidos, e
  `transcribe --model large-v3-turbo` usou o modelo escolhido.
- AppImage testado com um `ffmpeg` falso no `PATH` (prova que usa o embutido) e GUI abrindo/fechando com
  exit 0.

## Licença

AGPL-3.0-or-later. FFmpeg e whisper.cpp vão com suas licenças em `usr/share/licenses/`.
