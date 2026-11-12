# Decisões

Registros em ordem cronológica. Datas em UTC.

## 2026-11-11 — Escolha do caminho D: construir do zero
- **Decisão:** implementar o editor descrito em `docs/BRIEFING.md` neste repositório; a pesquisa do Passo 0 não é uma solicitação para o usuário testar Concat nem substitui a construção.
- **Motivo:** o usuário esclareceu que quer que o agente construa o editor. O objetivo é um editor útil para os próprios vídeos, não paridade de features.
- **Consequência:** Concat e outros projetos servem apenas como referência pesquisada; não copiar código, shader ou asset sem aprovação explícita.

## 2026-11-11 — UI padrão mantida: egui/eframe com wgpu
- **Decisão:** manter o default do briefing para a UI da Fase 1.
- **Motivo:** janela desktop Rust, UI customizada e caminho para compartilhar o device wgpu; sem evidência atual para justificar outra opção.
- **Consequência:** provar o compartilhamento de device na Fase 1 antes de depender dele nas fases de mídia.

## 2026-11-11 — Versões iniciais de eframe/egui e Rust
- **Decisão:** iniciar com eframe/egui 0.37.0 e toolchain Rust 1.95.0 (edition 2024).
- **Motivo:** consulta à fonte upstream do egui em 11/11/2026 confirma a release 0.37.0 e `rust-version = 1.95`; a API atual expõe o render state wgpu e callback customizado necessários ao critério do quad.
- **Limitação:** não houve resolução Cargo nem compilação neste ambiente; versões e APIs só foram conferidas na fonte GitHub, então a validação real ainda depende de Rust/Cargo disponíveis.

## 2026-11-11 — Demais defaults e arquitetura mantidos
- **Decisão:** manter os defaults descritos na seção 7. A integração FFmpeg via subprocesso no crate `editor-media` é um protótipo operacional, não a decisão final de backend.
- **Motivo:** permite testar probe, seek isolado e export sem bindings nativos enquanto construímos o fluxo; não há ffmpeg/ffprobe local para medir precisão, latência ou estabilidade.
- **Limitação:** não satisfaz o spike da Fase 2A nem comprova preview contínuo. A escolha entre bindings e processo continua aberta até medições reais em FFmpeg 7+ e consulta ao usuário.

## 2026-11-11 — Nome D-cut e ícone fornecido pelo usuário
- **Decisão:** nome do app/projeto: `D-cut`. Crates usam o prefixo provisório `editor-`, concentrado em um único lugar para facilitar renomear código no futuro. Usar a imagem de ícone gerada por IA e enviada pelo usuário, corrigindo o lettering para `D-cut`.
- **Motivo:** o usuário definiu o nome como D-cut e esclareceu que criou o ícone com IA; o texto da imagem estava incorreto. A arte de referência está em `assets/branding/d-cut-logo-reference.webp` e a versão corrigida em `assets/branding/d-cut-icon.png`.

## 2026-11-11 — Referências visuais iniciais
- **Decisão:** usar as imagens enviadas pelo usuário como referência de layout: biblioteca à esquerda, preview ao centro, inspector/export à direita e timeline embaixo; tema escuro e acentos azul/violeta/teal. Adaptar a UI para 1366×768 e implementar a aparência completa gradualmente.
- **Motivo:** são referências claras para a experiência desejada; a casca da UI agora ocupa esse layout, mas ainda não está conectada ao fluxo de mídia.

## 2026-11-11 — GitHub, branch e publicação
- **Decisão:** desenvolvimento local no checkout e na branch Arena existentes. GitHub é apoio para colaboração, documentação upstream e CI; não é dependência de runtime nem requisito para desenvolver ou usar o editor. Sem push, force-push, release ou pacote publicado sem pedido explícito.
- **Motivo:** o usuário corrigiu que GitHub é apenas uma ajuda; o produto deve funcionar localmente.

## 2026-11-11 — Execução contínua das fases autorizada pelo usuário
- **Decisão:** executar todas as fases em sequência sem pedir confirmação entre fases, fazendo o máximo possível e listando com precisão o que ficar pendente.
- **Motivo:** pedido explícito do usuário em 2026-11-11.
- **Consequência:** o fluxo normal de pausa no fim de cada fase fica suspenso para este trabalho em lote. Continuam valendo honestidade sobre verificações e bloqueios, a necessidade de não inventar APIs, e a proibição de push/publicação sem pedido explícito.

## 2026-11-11 — Repositório git local recriado
- **Decisão:** o ZIP `D-cut-editor-workflow-unvalidated.zip` veio sem `.git`; foi criado um repositório local novo (branch `master`) com o snapshot importado como primeiro commit. A branch Arena citada em AGENTS.md não existe neste checkout.
- **Consequência:** sem push/remote. Se o histórico original existir em outro lugar, os commits daqui podem ser reaplicados por cima.

## 2026-11-11 — FFmpeg por processo externo para preview e export (provisório, aguarda confirmação)
- **Decisão:** decode de prévia, áudio de prévia e export usam processos `ffmpeg` com um **único construtor de grafo de filtros** (`editor-media::timeline`). Prévia e export compartilham o mesmo grafo; mudam só resolução, formato de pixel e encoder. O wgpu (`editor-render`) apresenta o frame numa textura do device do egui.
- **Dados medidos:** FFmpeg 9.0.2 do sistema; 20/20 seeks aleatórios frame-exatos (diferença média < 2/255 contra decodificação sequencial de referência, sempre menor que a do frame vizinho); 60 s-ish de playback 1080p30 no Vega 8: 0 frames perdidos, 0 underruns; export frame-exato (contagem de frames do ffprobe = frames da timeline).
- **Divergência do briefing (§6):** o briefing quer composição na GPU pelo `editor-render`. Hoje a composição (escala, letterbox, gaps, concat, mix de áudio) é feita no FFmpeg. Funciona para corte/montagem, mas efeitos/texto na GPU (Fases 3–4) vão exigir mover a composição para wgpu ou usar bindings. **Precisa da decisão do usuário antes da Fase 3** (o briefing pede para perguntar antes de fixar o backend FFmpeg).

## 2026-11-11 — Seletor de arquivos próprio em egui (sem rfd por enquanto)
- **Decisão:** seletor de arquivos interno (`editor-ui/src/files.rs`), sem dependência nova; funciona sem xdg-desktop-portal/GTK/KDE. Arrastar-e-soltar também funciona. rfd/portal pode ser adicionado depois como opção.

## 2026-11-11 — Clipes vinculados e caminhos relativos no projeto
- **Decisão:** `Clip.link_group` (opcional, `serde(default)`) liga vídeo e áudio do mesmo arquivo; split/trim/move/delete agem no grupo, e "Desvincular áudio" separa. Compatível com projetos v1 antigos (campo ausente = sem vínculo), então `format_version` continua 1.
- **Decisão:** ao salvar, caminhos de mídia viram relativos à pasta do `.dcut`; ao abrir, são resolvidos de volta. Faixas padrão: V1, A1, A2 (áudio puro vai para a última faixa de áudio).
- **Decisão:** a primeira mídia importada num projeto vazio define proporção/resolução (lado maior ≤ 1920, sem upscale) e FPS do projeto (comando com undo).
- **Limitação:** FPS do projeto só pode mudar antes de importar mídia (posições são em frames).

## 2026-11-11 — Áudio de saída com cpal
- **Decisão:** `cpal 0.18` (default do briefing) para tocar o áudio; o FFmpeg já entrega PCM na taxa do dispositivo, então `rubato` não foi necessário. Relógio mestre = amostras consumidas pela placa.
- **Limitação:** a latência do buffer de saída não é compensada (áudio pode soar alguns ms depois do frame).

## 2026-11-11 — AppImage gerado no host, sem FFmpeg embutido
- **Motivo:** Docker existe mas o usuário não está no grupo `docker` e não há Podman; sem sudo não dá para usar o container Ubuntu 22.04.
- **Decisão:** `scripts/build-appimage.sh --host` monta o AppImage na máquina atual (appimagetool baixado em `build/tools/`). O binário exige glibc 2.44 (Arch atual); não é portátil para distros antigas. O FFmpeg é o do sistema (AppRun usa `usr/bin/ffmpeg` se um dia for embutido).
- **Pendente:** build no container e FFmpeg 7+ GPL próprio, como pede o briefing §9.

## 2026-11-11 — Licença de fonte Ubuntu-font-1.0 no deny.toml
- **Decisão:** `epaint` (egui) embute a fonte Ubuntu (licença Ubuntu-font-1.0) via `default_fonts`; adicionada ao allow-list para o `cargo deny` não falhar. **Revisar** compatibilidade de distribuição com AGPL (é licença de fonte, não de código); alternativa: desligar `default_fonts` e embutir uma fonte OFL.

## 2026-11-11 — Composição no FFmpeg APROVADA como provisória (divergência registrada)
- **Decisão do usuário:** composição/prévia/export via grafo FFmpeg aprovada **só como solução provisória**. É **divergência** do objetivo do briefing (§6: composição na GPU via `editor-render`/wgpu) e **não pode ser declarada definitiva** sem uma comparação medida (qualidade, desempenho na RX 6650 XT e no Vega 8, latência de scrub, viabilidade de efeitos/texto) contra a composição em wgpu.
- **Consequência:** antes de efeitos/texto na GPU (Fases 3–4) é preciso o comparativo ou a migração para wgpu.

## 2026-11-11 — AppImage: build de teste sem FFmpeg; entrega portátil final exige FFmpeg 7+ embutido
- **Decisão do usuário:** o build portátil de teste no container (Podman rootless, sem grupo docker) pode sair sem FFmpeg. O AppImage **não é considerado finalizado** enquanto depender do FFmpeg do sistema: a entrega portátil final precisa de FFmpeg 7+ GPL próprio (x264, x265, SVT-AV1, VAAPI, sem nonfree) e licenças documentadas em THIRD_PARTY_LICENSES.
- **Script:** `scripts/build-appimage.sh` usa Podman com `--userns=keep-id`; o Dockerfile inclui `libasound2-dev` (cpal).

## 2026-11-11 — Métrica de deriva A/V
- **Decisão:** "drift" = |média do offset vídeo−relógio nos últimos ~10 s − média nos primeiros 10 s|. O offset por frame é limitado pela quantização de 1 frame e não mede deriva. O relógio de áudio é interpolado entre callbacks (máx. 50 ms). Medido: 3,4 ms em 63,8 s (1080p30, Vega 8).

## 2026-11-11 — Fase 3: texto/legendas com wgpu + cosmic-text (escolha (a) do usuário)
- **Decisão:** `editor-text` (cosmic-text 0.19, fonte embutida Noto Sans Bold, SIL OFL 1.1, em `assets/fonts/`) gera a camada de texto RGBA com contorno; `editor-render` compõe base + texto com `compose.wgsl`, o **mesmo shader** na prévia (callback egui no device do eframe) e no export (`OffscreenCompositor` num device headless ou no device da UI). `editor-export` (crate prevista no §6) orquestra: FFmpeg decodifica o grafo da timeline em RGBA → GPU compõe → FFmpeg codifica (stdin) com o áudio do mesmo grafo. Projetos sem texto continuam no caminho só-FFmpeg.
- **Multi-faixa:** faixas de vídeo/áudio adicionáveis; a faixa de vídeo mais acima cobre as de baixo (prioridade, sem transparência/PiP ainda); áudio de todas as faixas é mixado. Faixa de texto T1.

## 2026-11-11 — Comparação medida: composição FFmpeg-only × wgpu (não é decisão final)
- **Cenário:** 1080p30, 20 s (testsrc2 + seno), x264 `veryfast`, notebook Ryzen 5 3500U / Vega 8 (RADV), FFmpeg 9.0.2 do host.
- **Velocidade:** FFmpeg-only 79,8 fps (7,5 s); caminho wgpu forçado 32,5–37,7 fps (16–18 s). VAAPI: FFmpeg-only 96 fps, wgpu 36,5 fps. O gargalo do caminho wgpu é o tráfego RGBA (decode→upload→readback→stdin, ~8 MB/frame), não o shader.
- **Qualidade:** PSNR entre as duas saídas = 43,2 dB (ruído de encoder; export vs fonte = 40,5 dB). Teste automático exige ≥ 38 dB com fonte colorida.
- **Bug encontrado na comparação:** `scale=in_color_matrix=bt709`/`out_color_matrix=bt709` no FFmpeg 9 não fecha a ida/volta YUV→RGBA→YUV (25,8 dB). Correção: mesma matriz (BT.601) nos dois sentidos do RGBA intermediário (≈48 dB, valores YUV preservados, arquivo marcado BT.709). **Limitação:** a prévia converte com BT.601 (desvio leve de cor só na tela).
- **Conclusão provisória:** a composição FFmpeg continua provisória para vídeo puro; o wgpu já é o caminho de texto. Antes de declarar qualquer um definitivo falta: medir na RX 6650 XT, levar a conversão YUV↔RGB para o shader (elimina o tráfego RGBA e o problema de matriz) e repetir esta tabela.

## 2026-11-11 — FFmpeg 7.1.5 embutido no AppImage portátil
- **Decisão:** FFmpeg 7.1.5 (tarball oficial, assinatura GPG verificada com a chave de release `FCF986EA15E6E293A5644F10B4322F04D67658D8`, SHA-256 `de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f`) compilado no container Ubuntu 22.04 com `--enable-gpl --disable-nonfree --disable-autodetect` + libx264, libx265, libsvtav1, VAAPI, libdrm, zlib. Bibliotecas de codec e libav* vão em `usr/lib` (rpath `$ORIGIN`); libva/libdrm/libz/glibc ficam do host (excludelist). Licenças e manifesto de build em `usr/share/licenses/ffmpeg/`.
- **Rede do Podman:** rootless `pasta` falhou (`/dev/net/tun` ausente); o script usa `--network=host` no build e no run.

## 2026-11-11 — Exceção cargo-deny: RUSTSEC-2026-0192 (ttf-parser unmaintained)
- **Decisão:** ignorar no `deny.toml` o aviso "unmaintained" do `ttf-parser` (vem de cosmic-text → fontdb; não há versão alternativa). O D-cut só lê a fonte embutida (nunca fontes do usuário/sistema), então não há entrada não confiável. Reavaliar quando cosmic-text/fontdb trocarem de parser.

## 2026-11-11 — Fase 4: motor de efeitos
- **Pacote:** `crates/editor-effects/effects/<id>/{manifest.json, effect.wgsl, golden.rgba}`. O `build.rs` embute todo diretório encontrado; adicionar um pacote não muda código (provado pelo commit `91c76e4`, só arquivos do `invert`).
- **Contrato do shader:** o pacote define `fn effect_main(uv) -> vec4<f32>`; um prelúdio fixo fornece `src(uv)`, `param(i)` (até 16 parâmetros), `texel()`, vertex shader e bindings. Validação com naga (`naga 30.0.1`, `wgsl-in`, puro Rust) antes de qualquer GPU; manifest inválido → `EffectError` com mensagem (testado: JSON quebrado, id inválido, categoria desconhecida, min ≥ max, default fora da faixa, shader que não compila, sem `effect_main`, id duplicado).
- **Render:** `editor-render::EffectRunner` (ping-pong, um pipeline por pacote) roda antes da camada de texto, no export (`OffscreenCompositor`) e na prévia (gravado no encoder do egui) — mesma cadeia. Efeitos são por clipe (`Clip.effects`, compatível com projetos antigos); clipe com efeito usa o caminho de export GPU.
- **Golden:** imagem procedural 96×64, `golden_params` do manifest, referência RGBA crua (sem dependência de PNG). Tolerância `D_CUT_GOLDEN_TOLERANCE` (2 local, 8 no CI/lavapipe); `D_CUT_UPDATE_GOLDEN=1` regenera; folha de contato revisada visualmente antes do commit. O teste também falha se o efeito praticamente não muda a imagem.
- **Desempenho medido (Vega 8, não a RX 6650 XT):** projeto 1080p30, 2 efeitos (gaussiano + vinheta), prévia 960×540, 63,8 s → 0 frames perdidos, 0 underruns, deriva 2,7 ms.

## 2026-11-11 — Fase 5: legenda automática (aprovado "5a" + modelo small)
- **Decisão:** whisper.cpp **v1.9.5** compilado no container (CPU, AVX2/FMA/F16C — exige CPU x86-64 com AVX2, ex. Zen/Haswell+; sem OpenMP, estático) e empacotado como `whisper-cli`, chamado como processo separado (como o FFmpeg). Sem bindings C++ no build Rust.
- **Modelo:** `ggml-small.bin` (multilíngue, 487.601.967 bytes, SHA-256 `1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b` conferido na API do Hugging Face), licença MIT (pesos da OpenAI, MIT). Licenças registradas em THIRD_PARTY_LICENSES **antes** do primeiro download. Download só por ação explícita do usuário, via `curl` do sistema (sem dependência HTTP nova), em `~/.local/share/d-cut/models/`, verificado por SHA-256 antes de usar; fora do AppImage.
- **Timestamps por palavra:** `whisper-cli -l pt -ml 1 -sow -oj` (um segmento por palavra); as palavras são agrupadas em legendas (≤ 42 caracteres, ≤ 3,5 s, quebra em pontuação/pausa). A transcrição entra como `ReplaceTextOverlays` (undo/redo).

## 2026-11-11 — Fase 6 (subfases A–D aprovadas pelo usuário)
- **6A:** `Clip.speed` constante (0,25–4×; vídeo `setpts`, áudio `atempo` encadeado) aplicada ao clipe e ao par vinculado; keyframes de parâmetros de efeito dentro de `EffectInstance.keyframes` (frames relativos ao clipe, rebaseados em split/trim de cabeça; interpolação hold/linear/smooth). **Não feito:** curva de velocidade variável (rampa) — só velocidade constante.
- **6B:** `Clip.transition_in` (pacote + duração) centrado no corte entre clipes adjacentes; um *stream lateral* (o outro clipe na janela, gaps fora) roda em paralelo ao principal e a GPU mistura com pacotes de transição (`category: transition`, prelúdio de duas entradas, `transition_main`). 16 pacotes com golden test + checagem de que progresso 0/1 dão origem/destino. Sem *handles* suficientes o quadro extremo é congelado.
- **6C:** `Clip.audio` (ganho dB, fade in/out, `afftdn`) como expressão `volume` no mesmo grafo do preview/export; faixas mixadas com `amix`.
- **6D:** proxies 540p (H.264, GOP 15, timestamps preservados) gerados em segundo plano em `~/.cache/d-cut/proxies/`, usados só na prévia; export sempre com originais.
- **Medição (notebook Ryzen 5 3500U/Vega 8, ~6 GB):** `d-cut-cli bench-preview` no projeto 720p com transição, efeito com chaves, texto e denoise: 163 fps a 640×360 (proxies), 149 fps sem proxies, 102 fps a 960×540; RSS ~97–100 MB. Playback na GUI com janela visível: 0 frames perdidos (medições anteriores). Uma rodada com a janela escondida pelo usuário (compositor estrangula frame callbacks) deu drops e foi descartada como inválida.

## 2026-11-11 — Fase 7: polimento
- **Autosave/recuperação:** `editor-project::autosave` grava `autosave-<pid>.dcut` + metadados a cada 30 s com mudanças (gravação atômica, mídia relativa). Saída limpa ou salvar apagam; ao abrir, autosaves de processos mortos são oferecidos (Recuperar/Descartar).
- **Atalhos:** acrescentados Q/W (cortar início/fim no playhead), ↑/↓ (pontos de edição), F1 (lista de atalhos), além dos existentes.
- **i18n:** catálogo `editor-ui/locales/en.tsv` (148 textos, chave = texto pt-BR); idioma em `~/.config/d-cut/settings.json` ou `D_CUT_LANG`. Teste falha se um `tr("…")` não tiver tradução. **Limitação:** mensagens de status montadas com `format!` (com números/nomes) e nomes de efeitos dos manifests continuam em pt-BR. Escolhido em vez de fluent para não adicionar dependência; trocar depois é mecânico (chaves já centralizadas).
- **Catálogo:** 45 pacotes (29 efeitos + 16 transições), todos com golden test; os 18 novos entraram só como arquivos de pacote.
- **Atualização (opt-in):** "Verificar atualizações" desligado por padrão; quando ligado, consulta a API de releases do GitHub via `curl` do sistema e só avisa. O AppImage leva `gh-releases-zsync|heronkil777-debug|D-cut|latest|…` e o `.zsync` é gerado pelo appimagetool, para AppImageUpdate/Gear Lever fazerem a atualização delta. Nada baixa sozinho.
- **AUR:** `packaging/aur/PKGBUILD` (compila do fonte; `sha256sums` a preencher na primeira tag) e `scripts/test-pkgbuild.sh`. Testado com `makepkg` local (sem sudo; `--nodeps` porque o cargo veio do rustup e não do pacman).
- **Templates e biblioteca de músicas (reavaliação):** continuam **fora**. Motivos: licenciamento de músicas/templates exige curadoria e redistribuição com licença compatível (CC0/CC-BY) e aumentaria muito o AppImage; o objetivo (cortar vídeo de celular e exportar) não depende disso. Reabrir se o uso real pedir — preferência por "pacotes de conteúdo" opcionais baixados sob demanda, como os modelos de IA.
