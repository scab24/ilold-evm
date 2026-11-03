# Findings Commands

Findings commands let you record security observations during an audit session. Findings are tied to the current session state and can be exported as a markdown report.

## finding

`fi [severity] [title]` or `finding [severity] [title]`

Records a security finding. Can be used in two modes:

**Inline mode** -- pass severity and title directly:

```
ilold[→ deposit → withdraw]> fi high Reentrancy in withdraw before balance update

  ✓ Finding F-001 added
```

**Interactive mode** -- run `fi` with no arguments to be prompted:

```
ilold[→ deposit → withdraw]> fi
  Severity (critical/high/medium/low/info):
  > high
  Title:
  > Reentrancy in withdraw before balance update
  Description (optional):
  > The external call on L38 occurs before totalStaked is decremented.
  ✓ Finding F-001 added
```

Valid severities: `critical`, `high`, `medium`, `low`, `info` (or `informational`).

The finding captures the current session sequence automatically. With no steps
in the active scenario it is still recorded, as a **session-level** finding (not
attached to a step); the REPL says so and the report lists its function as
`(session-level)`.

```
ilold[Staking]> fi low Missing events on admin setters

  ✓ Finding F-01 added (session-level: no step yet, not attached to a step)
```

## note

`n <text>` or `note <text>`

Attaches a free-text note to the current session step. Notes are included in the
exported report under **Notes**, with the sequence they were written after. With
no steps the note is session-level, as for findings.

```
ilold[→ deposit → withdraw]> n Check if msg.value can be zero here

  ✓ Note added (attached to step 1 (withdraw))
```

## scenario

`sc <subcommand>` or `scenario <subcommand>`

Scenarios are named step sequences of the active contract; `main` always exists.

| Subcommand | Effect |
|---|---|
| `scenario new <name>` | Create an empty scenario |
| `scenario list` | List scenarios with their step counts |
| `scenario switch <name>` | Make another scenario active |
| `scenario fork <name> [at N]` | Copy the active scenario (its first `N` steps) into a new one |
| `scenario delete <name>` | Delete a scenario (not the active one) |

Names match `^[a-z][a-z0-9_-]{0,31}$`. After `switch`, `fork` or `load`, the prompt
shows the active scenario and its steps, and Tab completes the new scenario names.

```
ilold[Staking → deposit → withdraw]> sc fork attack at 1

ilold[Staking → deposit → withdraw]> sc switch attack

ilold[Staking/attack → deposit]>
```

## status

`status <function> <status>`

Sets the review status for a function. Useful for tracking audit progress.

```
ilold[Staking]> status deposit reviewed

  ✓ Status updated
```

Valid statuses: `reviewed`, `suspicious`, `vulnerable`, `clean`, `inprogress`, `notreviewed`.

## findings

`fl` or `findings`

Lists the findings of the active scenario with id, severity, title and the
sequence (or function) they are attached to.

```
ilold[Staking]> fl

  2 finding(s) in Staking/main:
  F-01 [High] Reentrancy in withdraw before balance update  deposit → withdraw
  F-02 [Low] Missing events on admin setters  session-level
  → export writes them to a markdown report
```

## export

`ex` or `export`

Exports all findings, notes, and status changes as a markdown report. The file is
written to the current directory. The header names the project (the directory of
the Foundry root) and the session start time; all timestamps are UTC
(`YYYY-MM-DD HH:MM:SS UTC`).

```
ilold[Staking]> ex

  ✓ Exported to ilold-report-Staking.md
```
