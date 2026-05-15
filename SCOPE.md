# nixcfg — Scope Sheet

**Project:** nixcfg  
**Type:** Terminal UI for NixOS / nix-darwin / devShell flake configuration  
**Language:** Rust  
**Status:** Pre-development

---

## Problem Statement

Nix is one of the most powerful configuration systems available, but its buy-in cost is
prohibitive for most users. The documentation is fragmented, error messages are cryptic,
and there is no guided path from "I want to configure X" to a working flake. nixcfg
eliminates that barrier by providing a settings-menu-style TUI that reads and writes
valid Nix flakes — no Nix knowledge required to operate, fully hand-editable output.

---

## Goals

- Provide a classic settings-menu TUI (scroll up/down, toggle left/right, action bar at
  the bottom) that maps directly onto Nix option types.
- Support two primary modes: **system-wide** (NixOS or nix-darwin) and **directory
  flake** (devShell / project-local).
- Read existing `flake.nix` files, surface their options, and write back valid Nix after
  edits — preserving comments and hand-written sections.
- Ship a curated **flake registry**: a catalogue of popular flakes (home-manager,
  nix-darwin, nixvim, devenv, stylix, etc.) that users can add to their config in one
  keystroke.
- Never require the user to read Nix documentation to perform common tasks.

---

## Non-Goals

- A general-purpose Nix REPL or language editor.
- Remote / SSH host management.
- NixOS installer (separate concern).
- GUI / web interface (TUI only for v1).
- Automatic flake updates (`nix flake update` can be shell-exec'd as a helper action,
  but dependency resolution is out of scope).

---

## Modes

### System-wide mode
Targets `/etc/nixos/flake.nix` (NixOS) or `~/.config/nix-darwin/flake.nix` (macOS).
Exposes the full NixOS / nix-darwin option tree, grouped by module. Requires privilege
escalation for write (sudo prompt or polkit, not baked into the binary).

### Directory flake mode
Targets `./flake.nix` in the current working directory. Exposes devShell packages,
environment variables, pre-commit hooks, and devenv / flake-parts options.

---

## Option Type → Widget Mapping

| Nix type            | TUI widget                          |
|---------------------|-------------------------------------|
| `bool`              | Toggle (left/right or space)        |
| `enum`              | Inline select / radio list          |
| `string` / `path`   | Text input field                    |
| `int` / `float`     | Numeric input with optional bounds  |
| `listOf T`          | Multi-entry list editor             |
| `attrsOf T`         | Key-value pair editor               |
| `submodule`         | Nested settings page (drill-in)     |
| `nullOr T`          | Toggle to enable, then inner widget |
| `package`           | Package search (nixpkgs fuzzy find) |

---

## Architecture

```
┌─ TUI Shell (Ratatui) ───────────────────────────────────┐
│  Navigation · breadcrumbs · search · action bar          │
└──────────────┬──────────────────────────────────────────┘
               │ reads/writes
┌──────────────▼──────────────────────────────────────────┐
│  Config Model                                            │
│  ┌────────────┐  ┌──────────────┐  ┌────────────────┐   │
│  │Schema store│  │Config state  │  │Validator       │   │
│  │(option tree│  │(current vals,│  │(type-check,    │   │
│  │ types/docs)│  │ dirty track, │  │ nix eval dry   │   │
│  │            │  │ undo stack)  │  │ run on save)   │   │
│  └────────────┘  └──────────────┘  └────────────────┘   │
└──────────────┬──────────────────────────────────────────┘
               │
┌──────────────▼──────────────────────────────────────────┐
│  Nix Layer                                               │
│  ┌──────────────────────┐  ┌───────────────────────┐    │
│  │Flake parser          │  │Flake writer            │    │
│  │(rnix AST, option     │  │(AST mutate → fmt,      │    │
│  │ tree extraction)     │  │ preserve comments)     │    │
│  └──────────────────────┘  └───────────────────────┘    │
└─────────────────────────────────────────────────────────┘

  Side: Flake registry (bundled TOML catalogue of popular flakes)
```

---

## Flake Registry (v1 catalogue)

| Flake            | Description                              |
|------------------|------------------------------------------|
| nixpkgs          | Core package set                         |
| home-manager     | User-level dotfile and program config    |
| nix-darwin       | macOS system configuration               |
| devenv           | Per-project developer environments       |
| flake-parts      | Modular flake composition                |
| nixvim           | Neovim configured entirely in Nix        |
| stylix           | System-wide theming                      |
| sops-nix         | Secrets management                       |
| disko            | Declarative disk partitioning            |
| nixos-hardware   | Hardware-specific NixOS modules          |

Community flakes are pulled from a versioned registry TOML bundled in the binary and
updateable via `nixcfg registry update`.

---

## Key User Flows

### Flow 1 — New system config (NixOS)
1. `nixcfg init --system` scaffolds a minimal `flake.nix` in `/etc/nixos/`.
2. User opens nixcfg; sees top-level category list (boot, networking, users, services…).
3. Navigates into "services", toggles on "openssh", sets `permitRootLogin = false`.
4. Presses `s` to save. nixcfg writes the AST diff, runs `nixos-rebuild dry-run` to
   validate, reports any errors inline.
5. User presses `a` to apply (`nixos-rebuild switch`).

### Flow 2 — Add a flake from the registry
1. User presses `r` to open the registry browser.
2. Scrolls to `home-manager`, presses Enter.
3. nixcfg adds the input and module import to `flake.nix` and reloads the option tree.
4. New "home-manager" section appears in the menu.

### Flow 3 — Dir flake devShell
1. `cd my-project && nixcfg` — detects local `flake.nix`, opens in dir mode.
2. User adds packages from a searchable list, sets env vars, enables pre-commit hooks.
3. Save writes the flake; `nix develop` works immediately.

---

## TUI Keybindings (default)

| Key          | Action                        |
|--------------|-------------------------------|
| `↑` / `↓`   | Navigate options              |
| `←` / `→`   | Toggle / cycle value          |
| `Enter`      | Drill into submodule / edit   |
| `Esc`        | Back / cancel edit            |
| `/`          | Search options                |
| `r`          | Open registry browser         |
| `s`          | Save (dry-run + write)        |
| `a`          | Apply (`nixos-rebuild switch` / `darwin-rebuild switch` / `nix develop`) |
| `u`          | Undo last change              |
| `?`          | Help / show option docs       |
| `q`          | Quit                          |

---

## Out-of-scope for v1 / Future Work

- Multi-host configurations (flake outputs targeting multiple `nixosConfigurations`)
- Secret value editing (sops-nix integration is registry-add only in v1)
- Remote host apply over SSH
- Plugin / extension API
- GUI wrapper

---

## Success Criteria

- A user with zero Nix knowledge can install a package, enable a service, and apply a
  working NixOS configuration without reading any documentation.
- A developer can scaffold a working devShell for a new project in under two minutes.
- All written `flake.nix` files pass `nix flake check` without errors.
- Hand-written comments and custom expressions in existing flakes are preserved
  verbatim on round-trip.
