# Briefing completo do projeto

> Fonte: briefing integral fornecido pelo usuário. Atualizado em 2026-11-11 com a escolha do caminho D e o alinhamento aprovado. A especificação abaixo é mantida integralmente; decisões novas que alterem o escopo devem atualizar este arquivo e `docs/DECISIONS.md` juntos.


> **Divergências aprovadas (2026-11-11, ver docs/DECISIONS.md):** (1) a composição/prévia/export usa provisoriamente um grafo FFmpeg em vez do `editor-render`/wgpu; não é definitiva sem comparação medida contra wgpu. (2) O AppImage de teste pode sair sem FFmpeg; a entrega portátil final exige FFmpeg 7+ embutido com licenças documentadas.

## PROJETO: editor de vídeo open source estilo CapCut pro Arch Linux (AppImage)

**Escolha do Passo 0: D — construir do zero.** O relatório do Passo 0 foi apresentado; a pesquisa de alternativas não é uma solicitação para o usuário testar outro editor nem substitui a construção.

**Ordem de execução:** (1) relatório do Passo 0, sem código → pare; (2) mensagem de alinhamento → pare; (3) fases, normalmente uma por mensagem e só depois do meu “ok”. Se eu autorizar explicitamente várias fases numa mesma execução, siga sem pausas entre elas e registre tudo que ficar bloqueado para depois. Se a escolha do Passo 0 já estiver preenchida, pule o (1).

## 1. Papel e regras de trabalho

Você é meu engenheiro sênior de Rust, GPU (wgpu), FFmpeg e empacotamento Linux. Trabalhamos em fases pequenas e verificáveis.

### 1.1 Comunicação

- Português do Brasil, informal e direto, sem enrolação. Código, nomes, commits e comentários em inglês.
- Toda pergunta vem com a sua recomendação junto. No máximo 5 perguntas por mensagem, só as que realmente bloqueiam. Se não bloqueia, decide e registra em `docs/DECISIONS.md`.

### 1.2 Honestidade

- Nunca diga que compilou, testou ou rodou algo que não rodou. Separe sempre “verifiquei” (comando + resultado) de “não consegui verificar”.
- Se faltar toolchain, lib ou GPU no seu ambiente, avise na hora e me passe os comandos exatos pra eu rodar aqui.
- Pra qualquer coisa visual que você não consegue ver, me dê um checklist manual (“clique em X, deve aparecer Y”).
- Não invente API de crate. Na dúvida, confira docs.rs, cargo doc ou o código-fonte. Use `cargo add` (pega versão real), commite o `Cargo.lock`, fixe o toolchain em `rust-toolchain.toml` (edition 2024).

### 1.3 Ritmo

- Uma fase por mensagem. Não comece a fase N+1 sem meu “ok” explícito.
- Antes de codar uma fase, liste os critérios de aceite dela (as linhas numeradas da seção 10) pra eu confirmar que entendi.
- Se a fase estourar, pare e proponha a divisão em subfases antes de escrever código — não fatiar depois que já quebrou.
- Commits pequenos (`feat:`, `fix:`, `chore:`, `docs:`, `test:`), e uma tag local `phase-N` (ou `phase-Nx`) no fim de cada fase.

### 1.4 Segurança

- Nada de `sudo`, `pacman/yay`, `rm -rf` fora do repo, e não mexa em dotfiles. Se precisar de pacote do sistema, me passe o comando e eu rodo.
- Nada de `git push`, force-push, release ou publicação sem eu pedir.

### 1.5 Dependência e arquitetura

- Antes de adicionar dependência pesada (compila C/C++, passa de ~5 MB, ou muda a arquitetura) ou mudar arquitetura / decidir licença, responda em 3 linhas: o quê, por quê, alternativa. E espere.
- Dependência comum (`serde`, `thiserror`, `anyhow`, `tracing`, `proptest`, etc.) não precisa perguntar.

### 1.6 Travamento

- Travou 3 vezes no mesmo problema? Pare, explique o bloqueio e espere minha decisão. Nada de empilhar gambiarra.

### 1.7 Contexto entre sessões

Depois do meu “ok” no alinhamento (seção 11) e antes da Fase 1, crie:

- `docs/BRIEFING.md` — este documento inteiro, já com o que combinarmos (não resuma).
- `AGENTS.md` — regras e comandos de build/teste, curto (~80 linhas). Se eu usar Claude Code, crie também `CLAUDE.md` com apenas `@AGENTS.md`.
- `docs/DECISIONS.md` — cada decisão com o motivo e a data.
- `docs/STATUS.md` — fase atual, o que falta, bloqueios, próximo passo.

Regras: decisão nova que altere este documento atualiza `BRIEFING.md` e `DECISIONS.md` juntos. No começo de cada sessão, leia `AGENTS.md`, `docs/STATUS.md` e `docs/DECISIONS.md` antes de agir. Se `BRIEFING.md` e `DECISIONS.md` divergirem, `DECISIONS.md` vence e você me avisa.

## 2. Objetivo

Editor de vídeo desktop, open source e 100% local (sem conta, sem nuvem, sem telemetria), com a praticidade do CapCut: timeline multi-faixa, texto e legendas, filtros e transições na GPU, legenda automática offline e export até 4K. Roda no meu Arch (Wayland) e é distribuído como AppImage.

Motivo: o CapCut é proprietário, cobra assinatura pra tirar a marca d'água e sobe os vídeos pros servidores da ByteDance. Além disso não tem build nativo pra Linux.

Critério de sucesso real: eu abrir o editor, cortar um vídeo de celular e exportar em MP4 sem sentir falta do CapCut pro meu uso. Paridade de features é meio, não fim.

### Direção visual inicial (referência enviada pelo usuário em 2026-11-11)

Usar as imagens anexadas como referência de layout: biblioteca de mídia à esquerda, preview central, inspector/export à direita e timeline multi-faixa na parte inferior; interface escura, compacta e legível, com acentos azul/violeta e teal usados com moderação. Adaptar painéis e densidade para continuar utilizável em 1366×768. O visual completo será construído gradualmente; a janela vazia com quad da Fase 1 não representa a interface final. O nome do app é D-cut; usar o ícone gerado por IA e enviado pelo usuário, corrigindo o lettering para D-cut.

## 3. Passo 0 (obrigatório, antes de qualquer código)

Pesquise na web e confirme em fontes primárias (README, releases, issues, CONTRIBUTING, licenças). Me passe o link e a data de cada consulta. O que está escrito abaixo pode estar desatualizado: confirme, não confie. Se não tiver acesso à web, avise e me diga o que conferir na mão.

- Concat (`github.com/jub0t/Concat`): editor open source (AGPL-3.0-or-later), motor em Rust com compositor GPU. O README diz que existe build Linux (.AppImage, .deb, .rpm e pacote Arch). Confirme: versão e maturidade; se esses builds existem mesmo nas releases; issues abertas de Linux/Wayland/AMD; qual UI usa; layout de `src/`; como as contribuições são licenciadas (`CONTRIBUTING.md`, `LICENSING.md`, CLA/TRADEMARK se existirem); atividade recente (último commit, último release, issues respondidas).
- CapCraft for Android (`github.com/artcraft-android/capcraft-android`): derivado do Concat, AGPL-3.0-or-later, só Android. Referência de arquitetura e de lista de features, não de código.
- Outras alternativas abertas no Linux, uma linha cada: o que é, como está no Linux, licença, se está viva — OpenCut, FilmCraft (`storytold/filmcraft`), Cutlass (`1mrnewton/cutlass`), Drift (`CutWire-Studios/Drift`), WannaCut (`ter-9001/WannaCut`), Kdenlive, Shotcut, Olive, Flowblade, OpenShot.

Entregue em até 25 linhas:

- Tabela: item do escopo (seção 5) × Concat × outras (✅/⚠️/❌).
- As lacunas reais em relação ao que eu quero.
- Recomendação entre A só usar o Concat · B contribuir com PRs · C fork com mudanças · D construir do zero — com o que ganho e perco em cada um (tempo, controle, licença).

Pare e espere minha escolha. O resto deste documento descreve o caminho D. Se eu escolher A/B/C, adapte: A = guia de instalação no Arch + lista de lacunas pra abrir como issues; B = plano de contribuição (issues e PRs pequenos, regras do CONTRIBUTING, build local); C = plano de fork (como acompanhar o upstream, o que muda, créditos, licença, marca.

**Resultado registrado:** Passo 0 pesquisado e reportado em 11/11/2026. O usuário escolheu D ao esclarecer que quer que o agente construa o editor; não quer substituir o trabalho por um pedido para testar Concat. Não copiar código, shader ou asset de Concat/CapCraft sem aprovação explícita.

## 4. Minhas máquinas (ajuste se mudou)

- PC: AMD Radeon RX 6650 XT, Arch Linux, Wayland (Hyprland/MangoWC). Não dependa de GNOME/KDE nem de portal instalado: o seletor de arquivos precisa de fallback próprio.
- Notebook modesto (alvo mínimo, best-effort): Ryzen 5 3500U, Radeon Vega 8 (divide a RAM), ~6 GB de RAM, tela 1366×768. A UI (painéis + timeline) precisa ser usável nessa resolução.
- Não assuma o que a GPU suporta: confirme com `vainfo` e `vulkaninfo --summary`. Exemplo: a RDNA2 da RX 6650 XT não codifica AV1 em hardware.

## 5. Escopo (caminho D)

### 5.1 Dentro, por prioridade

Timeline multi-faixa com trim/split/ripple/undo-redo · export (MP4 H.264 primeiro; depois HEVC, VP9/WebM, AV1, GIF e PNG) · texto e legendas · filtros, efeitos e transições na GPU · keyframes e curvas de velocidade · áudio multi-faixa e redução de ruído · legenda automática local em PT-BR · chroma key, máscara e remoção de fundo · CLI headless pra render.

Desde cedo: projeto com proporção e fps configuráveis (16:9, 9:16, 1:1, 4:5) e UX básica de editor (snap na timeline, zoom, arrastar-e-soltar de arquivos, atalhos).

### 5.2 Efeitos: escalonamento explícito

Efeitos são escopo pleno, mas entram por fase — catálogo é expansível por design, e a meta é cobrir o meu uso real, não bater um número de concorrente.

- Fase 4: motor de efeitos + 10 pacotes de referência (2 por categoria: cor, blur, distorção, estilização, correção).
- Fase 6: +15 pacotes, incluindo transições.
- Fase 7: catálogo de 40–60, por demanda real de uso.
- Regra dura: pacote sem golden test não está pronto. Sem frame de referência, não conta como entregue.
- Teste de escalabilidade que vale mais que o número: adicionar o 11º pacote sem tocar em código do motor (só manifest + `.wgsl` + golden test).

### 5.3 Fora, por enquanto

Nuvem · contas · telemetria · IA generativa · biblioteca de templates · biblioteca de músicas · colaboração · Windows/macOS/mobile · plugins de terceiros · meta de paridade de catálogo com editores maduros (efeito entra por demanda, não por número).

## 6. Arquitetura (decidida)

Rust, workspace com até 11 crates. Motor separado da UI, efeitos como pacotes plugáveis, tudo local.

- `editor-core`: modelo do projeto (faixas, clipes, keyframes), comandos, undo/redo. Puro: sem I/O, sem GPU, sem UI.
- `editor-project`: serialização versionada, autosave, recuperação de crash.
- `editor-media`: decode/encode (FFmpeg 7+), thumbnails, waveform, proxies.
- `editor-render`: composição na GPU (`wgpu` + WGSL) e grafo de render. Recebe `Device/Queue` de fora: a UI compartilha a dela, a CLI cria um headless. Desenha também o texto (layout vem do `editor-text`), no preview e no export — nada de overlay da UI.
- `editor-effects`: catálogo de efeitos. Cada pacote = manifest (id, nome, categoria, parâmetros tipados com min/max/default) + shader `.wgsl`. Sem código nativo de terceiros; shaders validados com naga.
- `editor-text`: layout de texto, fontes, estilos, legendas.
- `editor-speech`: reconhecimento de fala offline.
- `editor-vision`: máscaras, remoção de fundo, tracking (ONNX Runtime).
- `editor-export`: perfis de export, fila de render, encode final.
- `editor-ui`: interface desktop.
- `editor-cli`: render/export sem interface, pra automação e testes.

Fluxo: importar → media (decode, thumb, waveform) → core (modelo) → render (GPU) → preview na UI.
Export: re-decode em qualidade cheia → render → encode.

Crie cada crate só quando a fase precisar — a Fase 1 cria apenas core, render, ui e cli. O nome público do app/projeto é **D-cut**, definido pelo usuário em 2026-11-11. O prefixo `editor-` dos crates é provisório: concentre-o num lugar só, pra renomear com um comando. O usuário confirma que criou o ícone enviado com IA e quer usá-lo; ajustar o lettering incorreto para `D-cut`. A versão ajustada está em `assets/branding/d-cut-icon.png`, com a imagem original preservada em `assets/branding/d-cut-logo-reference.webp`.

## 7. Decisões

### 7.1 Já tomadas (não reabrir)

Rust · FFmpeg pra codecs (nada de escrever codec) · `wgpu` + WGSL · efeitos em pacotes · 100% local · AppImage.

### 7.2 Defaults (diga no alinhamento se discorda)

- UI: egui/eframe com backend wgpu — um único device pra UI e render, timeline customizada é fácil, empacotamento leve. Alternativas: iced, GTK4. Evite Tauri: preview em tempo real via webview dá dor de cabeça e o AppImage com WebKitGTK fica pesado e frágil.
- Seletor de arquivos: portal (`rfd`) quando existir, com fallback pro seletor dentro do egui (ex.: egui-file-dialog, se existir e estiver mantido).
- FFmpeg no Rust: decidir num spike da Fase 2A entre bindings (`ffmpeg-next`, o fork `ffmpeg-the-third`, `rsmpeg`) e processo ffmpeg externo (só pro export). Critérios: suporta FFmpeg 7+, está mantido, dá seek preciso e acesso aos frames. Me pergunte antes de fixar.
- Áudio de saída: `cpal` (PipeWire/ALSA do sistema) + `rubato` pra resample.
- Texto: layout com `cosmic-text` (ou equivalente) e uma fonte padrão livre (OFL) embutida.
- Licença: AGPL-3.0-or-later (a mesma do Concat/CapCraft; compatível com FFmpeg/x264/x265 em modo GPL). Nada de `--enable-nonfree/libfdk_aac`: AAC com o encoder nativo do FFmpeg. Licença mais permissiva (MIT/Apache) exigiria FFmpeg LGPL (sem x264/x265) ou processo separado. Não copie código, shader ou asset do Concat/CapCraft sem eu aprovar; se copiar, mantenha créditos e licença.
- Codecs: importa tudo que o FFmpeg decodifica. Export H.264+AAC primeiro, com dois perfis: “Rápido” (VAAPI na AMD) e “Qualidade” (x264, CRF); depois HEVC (VAAPI/x265), VP9/Opus e AV1 por CPU (SVT-AV1). Saída padrão compatível: yuv420p, BT.709, +faststart, AAC 48 kHz.
- IA: só local. Legenda com whisper.cpp (PT-BR, timestamps por palavra); modelos baixados sob demanda, com checksum, fora do AppImage. Confira a licença de cada modelo (alguns de remoção de fundo são só pra uso não comercial) e registre em `THIRD_PARTY_LICENSES`. IA acelerada em GPU AMD no Linux é irregular (o suporte oficial do ROCm costuma não incluir a RX 6650 XT): baseline em CPU, Vulkan onde o runtime suportar. Remoção de fundo com ONNX depois.
- Templates e biblioteca de músicas: fora até a Fase 7.
- Projeto: JSON com `format_version` e migrações; mídia por caminho relativo + hash.
- Rede: nenhuma chamada além de download de modelos e checagem de atualização, ambos opt-in e desligados por padrão.
- Tamanho: AppImage < 150 MB sem modelos de IA.

**Alinhamento registrado em 2026-11-11:** nenhum default foi alterado. Foi mantido egui/eframe + wgpu para começar; a escolha do backend FFmpeg continua pendente até o spike da Fase 2A e depende de consulta ao usuário. Principais riscos discutidos: device/queue compartilhados; FFmpeg no AppImage (compatibilidade, tamanho, licenças); sync A/V e memória do notebook.

## 8. Regras de engenharia

- Todo edit do usuário é um Command com undo/redo; teste com proptest que aplicar e desfazer volta ao estado original.
- O mesmo caminho de render serve preview e export (muda só resolução/qualidade): o que eu vejo é o que exporta.
- O áudio é o relógio mestre do playback: o vídeo pula frames pra acompanhar, o áudio nunca atrasa.
- Decode e I/O em threads próprias, com cache de frames de orçamento de memória configurável (padrão conservador pro notebook). A thread da UI nunca bloqueia (meta: < 16 ms por frame).
- Trate mídia de celular como caso comum: rotação por metadata, VFR (a timeline roda no fps do projeto e escolhe o frame por timestamp), 10-bit/HDR (na dúvida, converta pra SDR BT.709 e me avise), matriz e range de cor corretos. Seek preciso por frame: decodifica a partir do keyframe anterior.
- HUD de debug ligável por flag (frame time, frames perdidos, memória): os critérios de desempenho dependem dele.
- Sem `unwrap/expect` em caminho de produção; `unsafe` só com comentário `// SAFETY:`; erros com `thiserror` (libs) e `anyhow` (binários); logs com `tracing` só em arquivo local, nada enviado. Textos da UI num catálogo (ex.: fluent) desde o começo, pra i18n não virar retrabalho.
- Testes: unitários no core; golden-image por efeito (frame de referência com tolerância; no CI use Vulkan por software/lavapipe com tolerância maior, local use a RX 6650 XT com a menor); integração com ffprobe conferindo duração, resolução, fps e codec do export.
- Mídia de teste gerada com FFmpeg (`testsrc2` + `sine`), incluindo um clipe vertical com rotação e um VFR. Nada de vídeo grande nem com copyright no repo.

## 9. AppImage e CI

- Build num container com glibc antiga (ex.: Ubuntu 22.04) pra rodar em mais distros — buildar direto no Arch gera um AppImage que só abre em distros com glibc igual ou mais nova. O alvo principal continua sendo o Arch. Entregue `scripts/build-appimage.sh` (podman ou docker) que sirva local e no CI.
- Não empacote Vulkan, Mesa, drivers VAAPI nem libs de áudio/display do sistema (ALSA, PulseAudio, PipeWire, Wayland, xkbcommon, libva...): use as do host (siga a excludelist do AppImage). Empacote um FFmpeg 7+ sob medida (GPL; x264, x265, SVT-AV1, VAAPI; sem nonfree) e registre as licenças em `THIRD_PARTY_LICENSES` (gere com cargo about/cargo deny + o manifesto do build do FFmpeg).
- `.desktop`, ícone e metadados AppStream corretos, pra aparecer nos launchers.
- Documente o requisito de FUSE e o fallback `--appimage-extract-and-run`.
- GitHub Actions: a cada push/PR, fmt, clippy, test e cargo deny; a cada tag, AppImage + `.zsync` + checksums (SHA-256). Auto-update via zsync é opt-in. PKGBUILD pro AUR só no fim.

## 10. Fases (caminho D)

Cada fase termina com o AppImage gerado (a partir da 1) e o relatório da seção 12. Os itens numerados são os critérios de aceite — o relatório responde um por um, na ordem.

### Fase 1 — Esqueleto

1. Workspace mínimo com core, render, ui, cli e `cargo build --workspace` verde.
2. `rust-toolchain.toml` fixado (edition 2024) e `Cargo.lock` commitado.
3. Janela vazia abre em Wayland e em X11, mostra a versão no título.
4. Um quad desenhado pelo editor-render aparece dentro da janela egui (prova o device wgpu compartilhado).
5. Fecha limpo: exit code 0, sem panic no log.
6. `scripts/build-appimage.sh` gera o AppImage.
7. CI verde (fmt, clippy `-D warnings`, test, cargo deny).
8. Tag `phase-1`.

### Fase 2 — Um vídeo, uma faixa

Cada subfase com relatório próprio; AppImage ao fim da 2D.

#### 2A — Decode + preview

1. Importa MP4 H.264.
2. Seek preciso por frame: 20 seeks aleatórios, nenhum erro > 1 frame.
3. Play sem áudio, a timeline no fps do projeto.
4. Thumbnails na timeline.
5. HUD de debug ligável por flag (frame time, drops, memória).
6. Clipe vertical com rotação por metadata e clipe VFR abrem corretos.
7. Spike do FFmpeg concluído e a escolha registrada em `DECISIONS.md` com os dados que a justificam.

#### 2B — Áudio

1. Waveform na timeline.
2. Play com áudio como relógio mestre.
3. A/V sync medido: drift < 20 ms em 60 s de play contínuo (número no relatório, medido pelo HUD/log).
4. Sem dropout nem underrun no play.

#### 2C — Edição

1. Split, trim, delete e ripple funcionando.
2. Undo/redo em todo edit, com proptest de roundtrip.
3. Salvar e abrir projeto JSON versionado, com teste de roundtrip.

#### 2D — Export

1. MP4 H.264+AAC nos perfis Rápido (VAAPI) e Qualidade (x264/CRF).
2. O mesmo export roda pela `editor-cli`.
3. ffprobe confere: duração ±1 frame, resolução, fps, codec, yuv420p, faststart.
4. AppImage atualizado.

### Fase 3 — Multi-faixa, texto e legenda manual

1. Timeline multi-faixa com pelo menos 2 faixas de vídeo e 2 de áudio.
2. Título e legenda `.srt` aparecem no preview.
3. Título e legenda saem queimados no export (nada de overlay da UI).
4. Projetos 9:16 e 1:1 abrem, editam e exportam com a geometria correta.
5. Texto renderizado pelo `editor-render` (mesmo caminho de preview e export).

### Fase 4 — Motor de efeitos na GPU + 10 pacotes de referência

1. Pipeline manifest → naga → grafo de render validado (manifest inválido falha com erro claro, não com panic).
2. 10 pacotes de referência, 2 por categoria (cor, blur, distorção, estilização, correção), cada um com golden test.
3. Golden test roda local (RX 6650 XT, tolerância menor) e no CI (lavapipe, tolerância maior).
4. Preview 1080p30 sem drops — HUD, 60 s de play, 2 efeitos empilhados — na RX 6650 XT.
5. Adicionar um 11º pacote sem tocar em código do motor (só manifest + shader + golden). Esse é o teste real de que o catálogo escala.

### Fase 5 — Legenda automática local (PT-BR)

1. whisper.cpp rodando offline em PT-BR com timestamps por palavra.
2. Download do modelo opt-in, com checksum, fora do AppImage.
3. A transcrição vira uma trilha de legendas editável no editor-core (é um Command como qualquer outro: undo/redo funciona).
4. Licença do modelo escolhido registrada em `THIRD_PARTY_LICENSES` antes de baixar.
5. Qualidade em PT-BR avaliada com um áudio meu e o resultado relatado (honesto: se ficou ruim, diga).

### Fase 6 — Keyframes, transições, áudio multi-faixa e proxies

Divida em subfases para implementação e validação. A regra de esperar “ok” é substituída quando o usuário autorizar explicitamente execução em lote. Cobertura mínima:

- Keyframes e curvas de velocidade.
- Transições (+15 pacotes da seção 5.2).
- Áudio multi-faixa: mixagem, fade, redução de ruído.
- Proxies.
- Aceite final (eu testo): abrir e editar 720p no notebook modesto sem estourar a RAM de ~6 GB.

### Fase 7 — Polimento

1. Autosave e recuperação de crash.
2. Atalhos completos.
3. i18n (PT-BR e EN).
4. Catálogo de efeitos em 40–60 (seção 5.2).
5. AUR (PKGBUILD) e auto-update via zsync opt-in.
6. Reavaliar templates e biblioteca de músicas.

## 11. Alinhamento (mensagem depois da minha escolha, caminho D)

Numa só mensagem:

- O que você entendeu do projeto, em 5 linhas.
- Quais defaults da seção 7 você mudaria, com o motivo.
- Os 3 maiores riscos técnicos e como pretende mitigar — pense em: sync A/V no preview, device wgpu compartilhado com a UI, FFmpeg dentro do AppImage, IA em GPU AMD, memória no notebook.
- Até 5 perguntas que realmente bloqueiam.

Sem código. Pare e espere meu “ok”.

**Status:** alinhamento enviado e aprovado pelo usuário em 2026-11-11. Confirmação: o usuário quer construção do editor; não quer ser orientado a testar Concat como substituto.

## 12. Definição de pronto (relatório de fim de fase)

Toda fase termina com tudo abaixo:

- `cargo fmt --all --check` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅
- `cargo test --workspace` ✅
- AppImage gerado (a partir da Fase 1) ✅
- Relatório, nesta ordem:
  1. Critérios de aceite da fase, um por um, com ✅ / ⚠️ / ❌.
  2. O que não funciona (sem maquiar).
  3. O que você não conseguiu testar + checklist manual pra eu rodar.
  4. Decisões novas (e o commit que atualizou `DECISIONS.md`).
  5. Próxima fase em uma linha.
- Tag `phase-N` criada.

Pare e espere meu “ok”, salvo quando o usuário autorizar explicitamente a execução contínua de várias fases; nesse caso, conclua e relate o máximo possível antes de pausar.

## Acordos de colaboração registrados

- O usuário escolheu o caminho D ao dizer explicitamente que quer que o agente construa o editor. Não substituir isso por pedir que o usuário teste um projeto existente.
- Desenvolvimento permanece no checkout e branch Arena vinculados à sessão. Sem push, force-push, release, PR ou publicação sem solicitação explícita.
- A implementação inicial da Fase 1 foi criada após o usuário confirmar seus oito critérios. A validação Rust, a execução da janela, o AppImage e a CI ainda estão pendentes; consultar `docs/STATUS.md` para o estado atual.
