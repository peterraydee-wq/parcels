```
                          __  
   ___  ___ ____________ / /__
  / _ \/ _ `/ __/ __/ -_) (_-<
 / .__/\_,_/_/  \__/\__/_/___/
/_/                           
```

search winget, scoop, chocolatey, npm, pnpm, bun, pip, uv, pipx, cargo, go, vcpkg, dotnet tools and
GitHub repos **at once**, then install / upgrade / remove from one place.

## shape

```
parcels.exe        Go + Charm (bubbletea / lipgloss / bubbles v2)   the TUI
parcels-core.exe   Rust, std only, zero crates                       the engine
```

the TUI never touches a package manager. it spawns `parcels-core` and reads **JSON lines**, so the
engine is scriptable on its own: `parcels search ripgrep`, `parcels list`, `parcels outdated`.

## build (windows)

```powershell
.\build.ps1 -Install
```

needs Rust and Go 1.24+. the first build runs `go mod tidy`, which fetches Charm from charm.land.

## use

```
parcels                              open the TUI
parcels export > manifest.json       snapshot everything installed (also: `e` in the TUI)
parcels import manifest.json --pin   rebuild a machine from a snapshot
parcels run install npm typescript   one-shot, no TUI
```

keys: `tab` / `1-4` tabs · `space` select · `enter` install · `u` upgrade · `x` remove ·
`U` upgrade everything · `/` search or filter · `e` export · `t` task log · `m` mute · `?` help

managers that aren't installed are skipped. turn any of them off in the Providers tab.

**a manager isn't showing up?** run `parcels doctor`. it lists every manager, where it was found (or the
install hint if not), and how many directories were searched. detection checks your PATH, the *registry*
PATH (so tools installed after your terminal opened still work), and well-known install dirs: scoop shims,
cargo, bun, go, dotnet, vcpkg (`VCPKG_ROOT`), npm, pnpm and python.

set `PARCELS_ASCII=1` if your font has no Nerd Font glyphs (the 󰏖 icon becomes ▣).

## notes

- **pip / uv / pipx** can't fuzzy-search (PyPI has no search API), so they look up exact names.
- **go** searches GitHub for Go repos, or resolves a module path like `github.com/junegunn/fzf`.
- **git** clones into `~\parcels\repos` (override with `PARCELS_REPOS`). set `GITHUB_TOKEN` to lift search rate limits.
- ids and versions are validated and passed as argv entries, never through a shell.
- add a manager: implement `Provider` in `core/src/providers.rs`, add it to `all()`.

there are a few things hiding in here. words typed into search are a good place to start.
