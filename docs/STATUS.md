# Status do projeto

- **Data:** 2026-11-11 (UTC)
- **Produto:** D-cut; caminho D — construção do zero.
- **Branch:** `arena/9ca1c6b4-d-cut` (ativa). Não trocar nem renomear.
- **Escopo:** o usuário autorizou executar todas as fases em lote, sem pausas entre elas. Essa autorização suspende o fluxo normal de “ok” por fase, mas não muda os critérios de aceite nem autoriza publicação.
- **Estado honesto:** protótipo em progresso, nenhuma fase concluída. Fase 1 já não passou por build; trabalho subsequente abaixo é código não compilado.
- **Ícone:** arte gerada por IA e enviada pelo usuário; lettering ajustado para D-cut. Fonte `assets/branding/d-cut-logo-reference.webp`; versão corrigida `assets/branding/d-cut-icon.png`.

## Implementado no checkout (ainda sem validação de compilação/runtime)

- Workspace Rust edition 2024, toolchain declarada 1.95.0, regras cargo-deny e preferência do resolver por versões compatíveis com MSRV.
- `editor-core`: modelo de projeto/formato, assets, trilhas, clipes, overlays, animações/keyframes, transições e metadados de proxy; validação estrutural, split/trim/ripple-delete reversíveis e undo/redo; testes unitários e property test proposto.
- `editor-project`: JSON versionado com gravação por arquivo temporário/rename, validação de carga/salvamento e importador SRT com quantização a frames.
- `editor-media`: wrappers locais para ffprobe/ffmpeg, metadados de áudio/vídeo/rotação, extração isolada de frame RGBA com limite de memória, waveform RMS em streaming e montagem de export H.264/AAC (x264 e VAAPI). O subprocesso é provisório; precisão, desempenho e disponibilidade de FFmpeg 7+ não foram medidos.
- `editor-cli`: versão, validação de projeto, importação SRT, probe e export de fonte única.
- `editor-ui`: fluxo funcional inicial foi conectado: mídia importada entra no modelo de projeto; é possível adicionar clips às faixas de vídeo/áudio, selecionar, dividir no playhead, excluir sem ripple, undo/redo e abrir/salvar JSON pelo caminho informado no inspector (Ctrl+S/Z/Y e Espaço). Isso ainda não foi compilado/testado. A reprodução só move o playhead: não decodifica nem mostra vídeo/áudio; export da timeline ainda não está conectado.
- `scripts/test-media-workflow.sh`: gera um fixture local, roda probe/export pela CLI e checa o resultado com ffprobe quando as ferramentas existirem.
- AppImage/CI iniciais e documentação. AppImage atual não inclui FFmpeg 7+ e ainda não foi produzido.

## Verificações desta continuação

- TOML de workspace/configuração/manifests: parsing Python passou.
- Fontes Rust: parsing/formatação via `@scalar/rust-fmt` em WASM (rustfmt 1.99, edition 2024) passou. Isso **não** equivale ao `cargo fmt` da toolchain pinned, nem a `rustc`, Clippy ou testes.
- Scripts `build-appimage.sh`, `AppRun` e `test-media-workflow.sh`: sintaxe shell passou; XML AppStream e `git diff --check` também.
- Ferramentas presentes: `gcc`, Python e Node/npm. Ausentes: `rustc`, `cargo`, `rustup`, Podman, Docker, ffmpeg, ffprobe e GPU acessível.

## Pendências críticas — impedem entregar o AppImage/utilitário pronto

1. Gerar e versionar `Cargo.lock`; compilar com Rust 1.95+; executar fmt, clippy e testes e corrigir erros.
2. Rodar a integração de mídia real com FFmpeg 7+: seek por timestamp/frame, rotação, VFR, waveform e codecs; não há dados do teste ainda.
3. Implementar pipeline de preview contínuo e enviar frames para o renderer compartilhado; UI ainda é protótipo visual.
4. Validar o novo fluxo UI de edição/projetos no Arch; ainda faltam seletor nativo de arquivos, relink de mídia, edição por drag/trim/ripple e persistir a localização do projeto com caminhos relativos portáveis.
5. Render/export de projetos com múltiplas trilhas; os comandos presentes só exportam uma fonte recortada e não compõem overlays.
6. Reprodução de áudio sincronizada, waveform UI, mixagem, fade, redução de ruído e critérios de drift/dropout.
7. Texto e legendas no caminho de render/preview/export; parser SRT e modelo de dados não são renderização.
8. Motor naga de efeitos, 10 pacotes/golden tests e catálogo expandido. Tipos de keyframe/curva/transição/proxy já existem no core, mas ainda não há integração com render/playback nem implementação funcional dessas ferramentas.
9. Integração Whisper offline, gerenciador de modelos opt-in/checksum/licenças e avaliação com áudio fornecido pelo usuário.
10. Autosave/recuperação, catálogo i18n PT-BR/EN, atalhos completos, AUR e atualização zsync opt-in.
11. Empacotar FFmpeg 7+ sob medida e gerar avisos completos de licenças; compilar/testar AppImage em Ubuntu 22.04; testar abertura/FUSE/`--appimage-extract-and-run`.
12. CI realmente verde, validação de GPU AMD/Wayland/X11, desempenho na RX 6650 XT e teste manual no notebook de 6 GB.

## Próximo passo técnico

Disponibilizar Rust/Cargo 1.95+ para gerar o lockfile e compilar. Em seguida corrigir as falhas encontradas e continuar pelas pendências em ordem de dependência. Nenhum critério está marcado como aceito, nenhuma tag de fase foi criada, e nenhum commit/push/release foi feito.
