# Workspace Commands

Workspace commands handle session persistence and external tools.

## save

`save <name>`

Saves the current session (steps, findings, notes, statuses) to a JSON file under `~/.ilold/sessions/`.

```
ilold[→ deposit → withdraw]> save staking-audit

  ✓ Saved to /Users/you/.ilold/sessions/staking-audit.json
```

You can resume this session later with [load](#load), even across different ilold runs, as long as the same contract files are loaded.

The name is a plain file name: letters, digits, `-`, `_` and `.`, not starting
or ending with `.` (a trailing `.json` is accepted). Paths such as `../x`, `/tmp/x` or
`a/b` are rejected, so sessions always stay in `~/.ilold/sessions/`. Windows device
names (`con`, `nul`, `com1`, `lpt1`, ... with any extension) are rejected too. `save`
and `load` refuse a sessions directory or session file that is a symbolic link.

## load

`load <name>`

Loads a previously saved session from `~/.ilold/sessions/`.

```
ilold[Staking]> load staking-audit

  ✓ Session loaded: Staking (2 steps)
```

The prompt updates to reflect the loaded scenario and steps, and Tab completion
picks up its scenario names. The saved scenarios replace that contract's
scenarios; if the file belongs to another contract the REPL switches to it.
`load` accepts the same names as `save`.

## browser

`browser`

Opens the web UI. ilold starts an API server on startup; `browser` provides the URL.

```
ilold[Staking]> browser

  API running at http://127.0.0.1:52431/api/
```

## help

`?`, `h` or `help` lists the commands. Append `?` to any command for its usage,
for example `sl?`, `sc?` or `q?`. Tab completes command names, functions
(including inherited ones) for `c`, `i`, `tr` and `sl`, state variables for
`who`, `tl` and the second argument of `sl`, contracts for `use`, `deps` and
`usedby`, and scenario names for `sc switch|fork|delete`.

## quit

`q`, `quit`, or `exit`

Exits the REPL. `Ctrl+D` also exits. `Ctrl+C` only clears the line being edited.

Unsaved session data is lost on exit. Use [save](#save) before quitting if you want to resume later.
