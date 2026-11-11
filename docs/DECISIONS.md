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
