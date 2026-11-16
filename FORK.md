# Fork workflow

`fork-main` is an opinionated fork of upstream Paneru
(`karinushka/paneru`). New features (animation work, IPC additions for
external tools, opinionated defaults) land here first so they can be
iterated on quickly. Bug fixes still go upstream.

## Remotes and branches

| Ref | What it is |
| :--- | :--- |
| `origin` | Upstream, `karinushka/paneru`. |
| `fork` | This fork, `anguswww/paneru`. |
| `origin/testing` | Upstream integration branch: the base for upstream PRs and what `fork-main` follows. |
| `fork-main` | This fork's main line: `origin/testing` plus fork-only features. |

## Where a change goes

- **Bug fix that upstream wants**: branch off `origin/testing`, open the PR
  upstream, then merge `origin/testing` into `fork-main` once it lands (or
  cherry-pick it straight away if you need it now).
- **Fork-only feature**: commit on a topic branch off `fork-main` and merge
  it, or commit directly on `fork-main`. Keep one feature per commit, so any
  of them can later be cherry-picked into an upstream PR.

## Keeping conflicts small

- Sync with `git merge origin/testing` and don't rebase. `fork-main` is pushed
  and other branches build on it.
- Extend protocol types additively: a new field gets `#[serde(default)]`, and
  a new behaviour gets a new enum variant (as `Request::SubscribeWith` and
  `Operation::VirtualNumberOnDisplay` do) instead of reshaping an existing
  one. Old clients and daemons keep working, and upstream refactors of the
  existing shapes merge cleanly.
- Put fork-only systems in their own functions and register them next to the
  upstream ones. Avoid editing the body of an upstream system.

## Fork-only features so far

- `displays` array, plus `tab_group`, `cell` and `cell_front` on each window
  in `query state` (see `QUERY_AND_SUBSCRIBE_FORMAT.md`).
- `paneru subscribe --state` pushes `snapshot` events.
- `window virtualnum <n> display <id>`.
- `paneru --no-launch …` for client commands.
