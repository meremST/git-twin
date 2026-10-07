# Compilation, installation et utilisation

Ce document reprend, verifie et complete la section 9 de `design.md` sur l'etat
reel du prototype. Les commandes ci-dessous ont ete executees sur l'environnement
decrit dans `status.md` (Rust 1.93.1 pinne par `rust-toolchain.toml`, Git 2.43).

## 1. Prerequis Linux

Necessaires : Git, un compilateur/linker C, Rust et Cargo geres par `rustup`.
Aucune dependance Node.js, Python, service distant ou acces root n'est requise
pour compiler ou lancer la TUI.

Exemple Debian/Ubuntu :

```sh
sudo apt-get update
sudo apt-get install --yes build-essential git curl ca-certificates
```

Installer `rustup` selon <https://rustup.rs/> (telecharger puis lire le script
avant execution si vous preferez eviter un `curl | sh` direct) :

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
less rustup-init.sh
sh rustup-init.sh
. "$HOME/.cargo/env"
rustup --version
git --version
```

A l'entree dans le repertoire des sources, `rustup` selectionne automatiquement
la version fixee par `rust-toolchain.toml` (composants `rustfmt` et `clippy`
inclus). Aucune toolchain nightly n'est necessaire.

## 2. Recuperer les sources

Ce depot est initialise localement (`git init` + commit initial) ; aucune URL
distante publique n'existe pour l'instant. Depuis la racine du projet :

```sh
cd /chemin/vers/gitui
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version
cargo --version
cargo fetch --locked
```

`Cargo.lock` est committe : un utilisateur n'a pas besoin de le regenerer.
L'acces reseau est necessaire pour recuperer la toolchain et les dependances,
sauf cache prepare au prealable.

## 3. Compiler et tester

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings   # voir note ci-dessous
cargo test --locked
cargo build --locked --release
./target/release/gitt --version
./target/release/git-tui doctor
```

**Note** : `cargo fmt --all -- --check` et `cargo clippy -- -D warnings`
echouent encore aujourd'hui sur des ecarts preexistants (formatage non
applique partout, lints de style type `collapsible_if`, essentiellement) dans
`app/`, `config/`, `domain/` et `git/` — voir **Next priorities** de
`status.md`. Ce n'est pas encore une porte de qualite passante ; le code
reste neanmoins compilable et teste. `cargo clippy --all-targets` (sans
`-D warnings`) permet de voir les memes diagnostics sans faire echouer la
commande. Appliquer `cargo fmt --all` sur l'ensemble du projet constitue un
changement large et independant (voir le point 4 de `status.md`) a traiter
dans sa propre revue plutot qu'en melange avec des changements fonctionnels.

Les binaires attendus sont `target/release/gitt` et `target/release/git-tui`.
Pour developper ou lancer depuis les sources sans installer :

```sh
cargo run --locked -- --repo /chemin/vers/depot --view history
cargo run --locked -- --repo /chemin/vers/depot --view worktree
cargo run --example preview -- --demo
```

Les deux premieres necessitent un terminal interactif ; utiliser un depot
jetable pour tester les mutations (jamais un depot de travail reel non
jetable comme fixture).

## 4. Installer pour l'utilisateur

Methode recommandee, depuis la racine des sources :

```sh
cargo install --path . --locked
gitt --version
git tui doctor
```

Les deux binaires sont places dans `$CARGO_HOME/bin` (`~/.cargo/bin` par
defaut). Verifier que ce repertoire est dans `PATH` (normalement deja fait par
`rustup`). Ne jamais lancer `cargo install` avec `sudo`.

Alternative sans gestion d'installation Cargo, apres compilation release :

```sh
install -d "$HOME/.local/bin"
install -m 0755 target/release/gitt "$HOME/.local/bin/gitt"
install -m 0755 target/release/git-tui "$HOME/.local/bin/git-tui"
"$HOME/.local/bin/gitt" --version
```

Ajouter `~/.local/bin` au `PATH` du shell si necessaire. Choisir une seule des
deux methodes pour eviter de lancer une ancienne copie ; verifier avec
`command -v gitt` / `command -v git-tui` et `gitt --version`.

## 5. Mise a jour et desinstallation

Pour mettre a jour depuis les sources : recuperer la revision voulue, relancer
les controles de la section 3, puis :

```sh
cargo install --path . --locked --force
gitt --version
```

Desinstaller une installation geree par Cargo :

```sh
cargo uninstall git-twin
```

Desinstaller une installation manuelle :

```sh
rm -- "$HOME/.local/bin/gitt" "$HOME/.local/bin/git-tui"
```

Ceci conserve configuration, etat et cache (`$XDG_CONFIG_HOME/git-twin`,
`$XDG_STATE_HOME/git-twin`, `$XDG_CACHE_HOME/git-twin`, avec les replis XDG
usuels) ; les supprimer volontairement si besoin. Ne jamais supprimer un depot
Git utilisateur.

## 6. Releases et depannage

La premiere release cible Linux x86_64. Aucun binaire statique ou portable
n'est promis sans test prealable ; `aarch64` ne sera ajoute qu'apres
compilation et execution validees sur cette cible.

| Probleme | Verification / resolution |
| --- | --- |
| `cargo` introuvable | Charger l'environnement rustup (`. "$HOME/.cargo/env"`) et verifier `PATH` |
| Linker `cc` absent | Installer le compilateur/linker systeme (`build-essential` sous Debian) |
| Echec de telechargement | Verifier reseau, proxy et certificats ; ne pas desactiver TLS |
| Lockfile incompatible | Utiliser la toolchain fixee et la revision complete ; ne pas effacer `Cargo.lock` |
| Glyphes incorrects | Configurer une Nerd Font locale ou utiliser `--icons ascii` |
| Souris indisponible | Tester hors tmux, verifier sa configuration ou utiliser `--no-mouse` |
| Mauvais binaire lance | Comparer `command -v gitt`, `command -v git-tui` et `gitt --version` |
| Terminal mal restaure | Apres sortie du processus, executer `stty sane` ou `reset`, puis signaler le bug |
| Git < 2.25 | Mettre a jour Git ; `MIN_GIT` est verifie au demarrage |

## Minimum Git et support macOS

- `MIN_GIT = (2, 25)` est verifie au demarrage (`src/git/repo.rs`). Teste a ce
  jour avec Git 2.43 uniquement ; la limite basse (2.25) n'a pas encore ete
  executee sur une installation reelle.
- Aucun test macOS n'a ete effectue (pas de machine macOS disponible dans cet
  environnement). Le code n'utilise pas d'API specifique a Linux en dehors de
  `/proc` ou equivalent ; une validation reste a faire avant de l'annoncer
  supporte.
