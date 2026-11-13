# Publicação — D-cut 0.1.1

Tudo o que é preciso para publicar está nesta pasta. Nenhum passo exige `sudo` na máquina de build.
A publicação em si (push, tag, release, AUR) é feita por quem tem acesso ao GitHub e ao AUR.

```
release/
├── README.md                    # este roteiro
├── RELEASE_NOTES-v0.1.1.md      # texto do release (usado pelo CI)
├── set-repo.sh                  # troca owner/repo em todos os lugares
├── fill-sha256sums.sh           # checksum do tarball da tag -> PKGBUILD
├── prepare-aur.sh               # gera .SRCINFO e a pasta pronta para o AUR
└── aur/
    ├── PKGBUILD                 # fonte canônica (compila do tarball da tag)
    ├── .SRCINFO                 # gerado; vai junto no repositório do AUR
    └── README.md                # detalhes do envio ao AUR
```

## 0. Confirme o repositório

O nome do repositório está embutido em `Cargo.toml` (campo `repository`), no metainfo do AppStream,
no PKGBUILD e na update info do AppImage. Se o seu repositório não for `heronkil777-debug/D-cut`:

```bash
release/set-repo.sh seu-usuario/seu-repo
git add -A && git commit -m "chore: point release metadata at seu-usuario/seu-repo"
scripts/build-appimage.sh          # reembute a update info com o repo certo
```

## 1. Suba o `master`

```bash
git remote add origin git@github.com:SEU-USUARIO/SEU-REPO.git   # se ainda não existir
git push -u origin master
```

O CI (`checks`) roda `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` com
`D_CUT_GOLDEN_TOLERANCE=8` (lavapipe), o fluxo de mídia com FFmpeg e `cargo deny check`.
**Só continue quando o job `checks` estiver verde.**

## 2. Crie a tag

A tag dispara o job `appimage` (build em container Ubuntu 22.04 + `.zsync`) e o job `release`
(publica o GitHub Release com os três artefatos, usando `release/RELEASE_NOTES-<tag>.md`).

```bash
git tag -a v0.1.1 -m "D-cut 0.1.1"
git push origin v0.1.1
```

Se a tag já estiver publicada, **não mova**: crie a próxima (`v0.1.2`, …) e mova `pkgver` antes.

## 3. Confira o release

- Assets presentes: `D-cut-x86_64.AppImage`, `D-cut-x86_64.AppImage.sha256`, `D-cut-x86_64.AppImage.zsync`.
- `sha256sum -c` do asset bate.
- `readelf -p .upd_info D-cut-x86_64.AppImage` mostra `gh-releases-zsync|SEU-USUARIO|SEU-REPO|latest|…`.
- Download do `.zsync` funciona (é o que AppImageUpdate/Gear Lever usam).

## 4. Preencha o checksum do PKGBUILD

```bash
release/fill-sha256sums.sh v0.1.0
git add release/aur/PKGBUILD release/aur/.SRCINFO
git commit -m "chore(aur): sha256 for v0.1.0"
git push
```

## 5. Envie ao AUR

```bash
release/prepare-aur.sh     # gera .SRCINFO e build/release/aur/
```

Depois siga `release/aur/README.md` (precisa de chave SSH cadastrada no AUR).
Confira antes de enviar: `makepkg --printsrcinfo` local e `namcap PKGBUILD` se disponível.

## 6. Teste final (máquina limpa ou container)

```bash
# AppImage
chmod +x D-cut-x86_64.AppImage && ./D-cut-x86_64.AppImage
./D-cut-x86_64.AppImage --cli validate-project algum.dcut

# AUR (depois de publicado)
yay -S d-cut && d-cut --version
```

Sem FUSE: `./D-cut-x86_64.AppImage --appimage-extract-and-run`.

## Reverter

- Release errado: `gh release delete v0.1.0 --yes` (a tag pode ficar) e publique `v0.1.1`.
- AUR errado: `git revert` no repositório do AUR e novo `pkgrel`.
- Nunca `push --force` em `master` nem em tag publicada; nunca sobrescreva um asset já baixado.

## Nunca fazer sem autorização explícita

- `git push --force` / `--force-with-lease`.
- Apagar ou mover tags publicadas.
- `rm -rf` fora do repositório.
- Publicar com o job `checks` vermelho.
