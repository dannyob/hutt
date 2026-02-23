# Split Inbox Design

## Overview

Split inbox partitions the inbox into named categories via mu queries. Each
split is a saved query scoped to the inbox. Messages matching any split are
excluded from the main inbox view and shown in their own virtual folder.

Inspired by Superhuman's split inbox feature.

## Data Model

```rust
pub struct Split {
    pub name: String,
    pub query: String, // mu query, auto-scoped to inbox
}
```

Persisted per-account as `splits.<account>.toml` in the hutt config directory:

```toml
[[splits]]
name = "Newsletters"
query = "list:* OR from:substack.com"

[[splits]]
name = "GitHub"
query = "from:notifications@github.com"
```

### App State

- `splits: Vec<Split>` — loaded from file
- `split_caches: Vec<HashSet<u32>>` — cached docids per split (parallel vec)
- `split_queries: HashMap<String, String>` — `"#name" → query` for folder resolution

## Folder Prefix

Splits use the `#` prefix: `#Newsletters`, `#GitHub`. This parallels `@` for
smart folders and `/` for maildir folders.

## Query Execution & Inbox Filtering

### Effective Query

The effective mu query for a split is always:

    maildir:<inbox_folder> AND (<user_query>)

The inbox scoping is applied automatically. The user only writes the
distinguishing part of the query.

### Cache Population

Split caches are populated eagerly at two points:

1. **Account load** (startup + account switch) — after initial inbox load
2. **After reindex** — when `poll_index_frame()` returns complete

The refresh flow:

1. For each split, run `mu find` with the effective query. Collect returned
   docids into the split's `HashSet<u32>`.
2. Build a combined `excluded_docids: HashSet<u32>` as the union of all caches.

### Inbox Exclusion

When the current folder is the inbox, filter envelopes client-side after
`mu.find()` returns, before `rebuild_conversations()`:

    envelopes.retain(|e| !excluded_docids.contains(&e.docid))

### Split Folder View

When viewing a split (`#name`), `build_query()` returns the effective query.
Results come directly from mu, consistent with the cached docids.

## Folder List

- Splits appear in `known_folders` with `#` prefix, after smart folders
- Visible in the folder picker, filterable
- NOT shown as move targets (splits are virtual partitions, not real folders)
- Top bar shows `#Newsletters` etc. when viewing a split

## Create / Delete / Undo

### Create

Reuse the existing smart folder two-phase creation flow:

- Phase 1: enter query, see live preview (subject lines + match count)
- Phase 2: name the split

Triggered via command palette ("Create Split") or keybinding. A
`creating_split: bool` flag on `App` distinguishes the flow — when true, the
smart folder create UI saves to splits instead.

### Delete

When viewing a `#split`, the delete-folder keybinding removes it from `splits`,
saves the file, pushes `UndoAction::DeleteSplit { split }` to the undo stack.

### Undo

New `UndoAction::DeleteSplit` variant restores the split, re-saves the file,
and re-populates the cache. Parallel to `DeleteSmartFolder`.

## Overlapping Queries

Splits may overlap — a message can appear in multiple splits. No ordering or
deduplication between splits. The inbox excludes anything matching *any* split.
Users can write non-overlapping queries if they want clean separation.

## Edge Cases

- **Docid changes after moves**: Caches become stale for moved messages. This
  is fine — moved messages leave the inbox. Caches rebuild fully on reindex.
- **Conversations mode**: Exclusion is per-message before `rebuild_conversations()`.
  A thread may appear in both inbox and a split with different messages visible.
- **Filters**: Unread/starred/needs-reply filters apply after split exclusion.
- **Smart folders**: Unaffected. `@` folders remain general-purpose, don't
  participate in inbox exclusion.
- **Multiple accounts**: Per-account files (`splits.fil.toml`). Caches cleared
  and rebuilt on account switch.
