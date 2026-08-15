# Shell Command

Run shell commands from the launcher in the **background** — no terminal window is opened.

Type `/cmd <command>` in the launcher and press Enter. The command is launched detached
(fire-and-forget) and keeps running even after Noctalia exits.

## How it works

- Commands run through your own interactive shell (`$SHELL -ic`), so aliases, shell
  functions and login-shell environment are available — a bare `-c` would ignore them
  and break things like fish aliases.
- Execution is fire-and-forget: **no output is echoed and there is no exit-code
  notification**. The process is fully detached from Noctalia (double-fork), so it is
  never time-limited — long builds, `rsync`, or other slow jobs run to completion.
- If launching fails (rare, e.g. the global async-command limit is reached), an error
  notification is shown.
- Your command is recorded in the plugin's command history (see below).

## Usage

```
/cmd <command>
```

Examples:

| Input | Effect |
| --- | --- |
| `/cmd ls` | Lists files in the default workspace (or the shell's current directory) |
| `/cmd cd ~/proj && git status` | Changes to `~/proj` first, then runs `git status` there |
| `/cmd cd ~/proj && cargo build` | Long-running build, detached — keep using your desktop |

> Note: a bare `cd <dir>` (without `&& <command>`) has no visible effect — the
> background shell exits immediately after changing directory. Combine navigation
> with a command (`cd <dir> && <cmd>`) or rely on the **Default Workspace** setting.

A leading `cd <dir> && <rest>` compound is honoured (quoted paths and `\ ` escapes
work), so navigation and a command can be combined in one launch. Otherwise, when a
**Default Workspace** setting is configured, commands run there first.

## Features

- **Live completions** — as you type, the plugin queries your shell's own completion
  engine (`fish -c 'complete -C "<query>"'`, falling back to bash `compgen -c`), so
  suggestions grow dynamically with your system. A single unambiguous completion is
  snap-applied to the run entry; multiple candidates are offered as suggestions.
- **Command history** — every executed command is recorded (most-recent first,
  deduplicated, capped at 100 entries) in the plugin's data directory
  (`noctalia.pluginDataDir()` / `history` file). Recent commands appear when `/cmd` is
  empty and as you retype prefixes.
- **Snippets** — configure frequently used commands; they are shown when `/cmd` is empty
  and can be filled into the input with Enter.

## Settings

- **Default Workspace** — working directory commands run in. Leave empty to use the
  shell's current directory.
- **Snippets** — commands shown when `/cmd` is empty. Each entry is one command.

## Notes

- The history file lives in the plugin data directory (per-plugin folder under
  `NOCTALIA_STATE_HOME`, default `~/.local/state/noctalia/`). Removing the plugin's
  data directory clears the history.
- An interactive shell without a tty may print a harmless job-control warning to
  stderr; like all other output it is discarded.
