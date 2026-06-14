# ved Backlog

Ideas captured but not yet scheduled. No commitment except where noted. ved is
a pure-stdlib, zero-dependency `ed` clone, and that posture is part of what the
project *is* — several items below are gated on it, and one is rejected by it
outright. See `PLAN.md` for the slice history and `SECURITY.md` for the support
policy.

---

## Queued

### Compound (base-anchored) offset addressing — `$-5`, `5-3`, bare `$-`

ved already has *current-relative* offsets (`+N` / `-N` off the current line —
these work). What it lacks is offsets that attach to a *preceding* address:
`$-5` (fifth line before the last), `5-3`, `/re/+1`, and chained forms. Today
`Address::Offset(isize)` always resolves against `buf.current()`, so after `$`
the trailing `-5` falls through to the command parser and errors. The original
address parser explicitly deferred this (`src/address.rs`, "Compound expressions
like `$-5` also wait") and it never shipped.

Doing it faithfully means restructuring the `Address` / `Spec` model so an offset
composes onto its base address — the general ed form, not a special case for `$`.
Current line stays the implicit base for a bare `+3` / `-1`.

Context: the sibling editor **nved** (a from-scratch Go editor, not an ed clone)
shipped the `$`-anchored slice of this in its v0.4.0 (`$-N` / `$+N`, clamped) to
power a `tail N` shorthand. The decision was to ship nved first and queue ved
separately, because ved is the ed authority and should add the *general* form
rather than the `$`-only subset. Independent implementations, no shared code.

This is a clean, self-contained address-parser job with no dependency gate to
clear — good standalone scope whenever ved is next picked up.

---

## High-impact, unscheduled

### Search addresses — `/pattern/`, `?pattern?`

The highest-remaining-impact item. The address parser handles line numbers, `.`,
`$`, and offsets, but not regex search. Adding it connects the existing BRE
engine (`src/bre.rs`) to the address resolver. If the pure-stdlib gate is ever
bent, this is the most likely reason.

### `u` — undo

Single-level undo. Requires snapshotting buffer state before a mutating command.

---

## Smaller / conditional

These matter mostly for stricter POSIX fidelity or if the BRE engine is ever
published as a standalone crate. Until then, most are weight not worth chasing.

- **`!command` shell escape** — run a shell command from the `:` prompt.
- **Empty pattern reuse in `s//new/`** — reuse the last search pattern. (Bare
  `s` already repeats the whole last substitute as of v0.1.5; this is the
  pattern-only case.)
- **`\(...\)*`** — star applied to a group, not just a single atom.
- **`\{n,m\}`** — bounded repetition in BRE.
- **POSIX character classes** — `[[:alpha:]]`, `[[:digit:]]`, etc.
- **`\<` `\>`** — word-boundary anchors (an ed extension).

---

## Gated on the pure-stdlib posture

These would go through *if* the zero-dependency rule were ever relaxed (the most
plausible trigger being search-address work above). `std` has no ergonomic signal
handling, so they need `libc` FFI or a dependency.

- **Ctrl-C aborts input mode instead of killing the process** — traditional ed
  catches SIGINT to drop input mode back to command mode without committing.
- **Input-mode commands inside `g` / `v`** (`g/foo/c`, `/a`, `/i`) currently
  no-op for the input half; a faithful fix buffers the input-mode body upfront.

---

## Rejected by project identity

### In-block input-mode editor — will not build in ved

A design spec (`~/Downloads/ved-inblock-input-editor.md`) proposed making the
`a` / `i` / `c` input block a cursor-navigable compose region. The concept is
sound, but it needs raw-mode terminal handling, an ANSI escape parser, and
SIGWINCH — none of it in std, all of it against what ved is. This is a stronger
"no" than the gated items above: it is rejected by identity even if `libc` were
on the table. That idea now lives in **nved**, the separate Go editor built for
exactly this.
