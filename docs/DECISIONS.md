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
