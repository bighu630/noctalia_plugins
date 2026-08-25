# Shell Command

Run shell commands from the Noctalia launcher. Two prefixes share one plugin:

| Prefix | Behaviour |
| --- | --- |
| `/cmd <command>` | Runs the command in the **background** (fire-and-forget, no terminal window) |
| `/sh <command>` | Opens a **real terminal** running the command (live output, TUI apps, native history) |

## Background mode — `/cmd`

Launch a command detached and keep using your desktop. Execution is fire-and-forget:
**no output is echoed and there is no exit-code notification**. The process is fully
detached from Noctalia (double-fork), so it is **never time-limited** — long builds,
`rsync`, or other slow jobs run to completion. If launching fails (rare, e.g. the
global async-command limit is reached), an error notification is shown.

## Terminal mode — `/sh`

Open the command in your default terminal (via `noctalia.runInTerminal`) — the
original upstream behaviour. You get live output, interactive/TUI programs and the
terminal's own history. The terminal is held open after the command finishes (`read`
prompt) and drops into a fresh interactive shell (aliases/functions load).

### `cd` folder browser (terminal mode)

`/sh cd` lists folders; select one to fill the input (`/sh cd path/`) and keep
drilling in, or take the "Open in:" row to open a terminal directly in the current
folder.

## Shared features

- **Live completions** — as you type, the plugin queries the shell's own completion
  engine (`fish -c 'complete -C "<query>"'`, falling back to bash `compgen -c`), so
  suggestions grow dynamically with your system. A single unambiguous completion is
  snap-applied to the run entry; multiple candidates are offered as suggestions.
- **Command history** — every executed command (in either mode) is recorded
  (most-recent first, deduplicated, capped at 100 entries) in the plugin's data
  directory (`noctalia.pluginDataDir()` / `history` file). Recent commands appear
  when `/cmd` or `/sh` is empty and as you retype prefixes.
- **Snippets** — configure frequently used commands; they are shown when the input is
  empty and can be filled with Enter.
- **Aliases & functions** — commands run through your own interactive shell
  (`$SHELL -ic`), so aliases, shell functions and login-shell environment work. (In
  background mode an interactive shell without a tty may print a harmless
  job-control warning to stderr; it is discarded with all other output.)
- **Combined `cd`** — a leading `cd <dir> && <cmd>` compound (quoted paths and
  `\ ` escapes work) navigates and executes in one launch; otherwise commands run in
  the **Default Workspace** when set.

## Settings

- **Default Workspace** — working directory commands run in. Leave empty to use the
  shell's current directory.
- **Snippets** — commands shown when `/cmd` or `/sh` is empty. Each entry is one command.

## Notes

- The history file lives in the plugin data directory (per-plugin folder under
  `NOCTALIA_STATE_HOME`, default `~/.local/state/noctalia/`). Removing the plugin's
  data directory clears the history.
- The two providers share the same on-disk history file; each keeps its own in-memory
  copy and refreshes from disk before recording.
