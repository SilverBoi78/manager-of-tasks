# AUR packages

Two packages, following the usual AUR convention:

| Package | What it does |
| --- | --- |
| `manager-of-tasks` | Builds the tagged release from source with cargo. |
| `manager-of-tasks-bin` | Installs the prebuilt x86_64 binary from GitHub Releases (fast, no Rust needed). |

Once published, Arch, CachyOS, EndeavourOS and Manjaro users can install with an AUR helper:

```sh
yay -S manager-of-tasks-bin     # or: paru -S manager-of-tasks-bin
```

## Publishing for the first time

1. Create an account at <https://aur.archlinux.org/register>.
2. Create an SSH key for the AUR and add it to your account (*My Account → SSH Public Key*):

   ```sh
   ssh-keygen -t ed25519 -f ~/.ssh/aur -C "aur"
   cat ~/.ssh/aur.pub           # paste this into the AUR website
   ```

3. Tell SSH to use that key for the AUR by adding this to `~/.ssh/config`:

   ```
   Host aur.archlinux.org
       IdentityFile ~/.ssh/aur
       User aur
   ```

4. Publish both packages:

   ```sh
   ./packaging/aur/publish.sh
   ```

   The first push creates the packages on the AUR.

## Releasing a new version

After tagging a new release on GitHub (and waiting for its Release workflow to finish):

1. In both `PKGBUILD`s, set `pkgver` to the new version and `pkgrel=1`.
2. Update the checksums (needs `pacman-contrib`): run `updpkgsums` in each package folder.
3. Optionally test: `makepkg -f` in each folder.
4. Run `./packaging/aur/publish.sh`. It regenerates `.SRCINFO` automatically.
