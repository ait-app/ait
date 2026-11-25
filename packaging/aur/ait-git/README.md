# ait-git

Arch Linux development package for [Ait](https://github.com/ait-app/ait).
Tracks the latest commit on upstream's `main` branch and compiles the Rust
daemon and desktop application from source. Electron and third-party native
dependencies follow the versions locked by upstream.

```sh
git clone https://aur.archlinux.org/ait-git.git
cd ait-git
makepkg -si
```

The package provides `ait` and conflicts with other providers, including
`ait-bin`. Installing it replaces the stable package. Launch it with `ait`
or the Ait desktop entry.

Builds require network access for Git, npm, Cargo, Electron and native
dependencies. Rustup users automatically use upstream's `rust-toolchain.toml`;
users of Arch's `rust` package need a sufficiently recent compiler. Use the
system Node.js or another current version that meets the build dependencies.

The package version is `YYYYMMDD.g<8-digit hash>`; the app and daemon display `git-YYYY-MM-DD-<8-digit hash>`.
Run `makepkg -si` again, or enable development-package updates in your AUR
helper, to fetch and build later commits. AUR itself stores the build recipe;
each installation compiles on the user's machine.

This desktop package can coexist with `aitd-bin` or `aitd-git`, which install the
independent `/usr/bin/aitd` command. Disable desktop management of the built-in
daemon before running a standalone service against the same data directory.
The date-based package version upgrades from the old `0.2.6.r874.g253cb2e`
recipe without an epoch. Only recipe changes are published to AUR; new upstream
commits are picked up by rebuilding.
