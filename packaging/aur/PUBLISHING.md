# Publishing SnowDraw to the AUR

1. **Put the source on GitHub** (or GitLab). Commit **Cargo.lock** too — the
   PKGBUILD uses `cargo fetch --locked`, so the build fails without it.
   Cargo.lock is created by your first `cargo build`.
2. **Tag a release**: `git tag v0.2.0 && git push --tags`.
3. Edit `PKGBUILD`: set your name/email in `# Maintainer:` and your repo in `url=`.
4. In a clean folder containing only the PKGBUILD:
   ```
   updpkgsums                       # fills in sha256sums (pacman-contrib)
   makepkg -si                      # build + install locally to test
   namcap PKGBUILD *.pkg.tar.zst    # lint (optional but recommended)
   makepkg --printsrcinfo > .SRCINFO
   ```
5. Create an account at https://aur.archlinux.org and add your SSH public key.
6. Publish:
   ```
   git clone ssh://aur@aur.archlinux.org/snowdraw.git aur-snowdraw
   cp PKGBUILD .SRCINFO aur-snowdraw/
   cd aur-snowdraw
   git add PKGBUILD .SRCINFO
   git commit -m "Initial upload: snowdraw 0.2.0"
   git push
   ```
   (If the name `snowdraw` is already taken, pick another and change `pkgname`.)

For later releases: bump `pkgver`, reset `pkgrel=1`, run `updpkgsums`,
regenerate `.SRCINFO`, commit, push.
