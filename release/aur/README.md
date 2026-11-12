# AUR — d-cut

O `PKGBUILD` desta pasta é a fonte canônica (compila do tarball da tag). O `.SRCINFO` é gerado:

```bash
release/prepare-aur.sh
```

## Primeiro envio

1. Cadastre uma chave SSH pública em <https://aur.archlinux.org> (My Account → SSH Public Key).
2. Crie o repositório local do AUR:

```bash
cd "$(mktemp -d)"
git clone ssh://aur@aur.archlinux.org/d-cut.git
cp /caminho/do/repo/release/aur/PKGBUILD /caminho/do/repo/release/aur/.SRCINFO d-cut/
cd d-cut
git add PKGBUILD .SRCINFO
git commit -m "Initial import: d-cut 0.1.0-1"
git push origin master
```

O `d-cut` precisa ser criado pelo próprio push (o AUR cria o repositório no primeiro envio, se o
nome estiver livre). Se o nome já existir e for de outra pessoa, escolha outro `pkgname`
(ex.: `d-cut-bin` para o AppImage) e ajuste o `PKGBUILD`.

## Atualizações

1. `release/fill-sha256sums.sh vX.Y.Z` (atualiza `pkgver` antes, se necessário).
2. `release/prepare-aur.sh`.
3. No clone do AUR: copie `PKGBUILD` e `.SRCINFO`, `git commit -m "upgpkg: d-cut X.Y.Z-1"`, `git push`.

## Regras que quebram o pacote

- `.SRCINFO` desatualizado em relação ao `PKGBUILD` → o AUR rejeita.
- `sha256sums` errado → `makepkg` falha para todo mundo.
- `pkgver` sem a tag correspondente no GitHub → o download 404.
- Dependências que não existem nos repositórios oficiais (não use AUR como dependência).
