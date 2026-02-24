# Tab Bar Design

## Overview

Replace the top bar with a clickable tab bar showing folder tabs, an account
switcher, and an overflow button. Tabs are scrollable, with Inbox pinned on
the left.

## Tab Data Model

### Config

Optional `tabs` field on each account:

```toml
tabs = ["/Inbox", "#lists", "#", "/Archive", "/Sent", "@"]
```

Wildcards:
- `"/"` — all remaining named maildir folders not explicitly listed
- `"#"` — all remaining splits not explicitly listed
- `"@"` — all remaining smart folders not explicitly listed

Default when omitted: `["/Inbox", "#", "/", "@"]`

### Resolution

`resolve_tabs(config_tabs, account_folders, splits, smart_folders) -> Vec<String>`

1. Walk config list in order
2. Literal entries included directly
3. Wildcards expand to remaining items of that type not already in the list
4. Deduplicate

Account's named folders (from config): inbox, archive, drafts, sent, trash, spam.

### App State

- `tabs: Vec<String>` — resolved tab list
- `tab_scroll: usize` — index of first non-Inbox tab visible in scroll window
- `tab_regions: Vec<TabRegion>` — click regions computed each frame
- `account_picker_selected: usize` — selection in account picker popup

## Tab Bar Rendering

Layout left to right:

```
[fil] /Inbox  #lists  #github  ▸/Archive  /Sent  /Trash  …
```

1. **Account badge**: `[name]` — cyan bold on dark bg. Always visible. Clickable.
2. **Pinned Inbox**: Always rendered after account badge, regardless of scroll.
3. **Scrollable tabs**: From `tab_scroll`, render tabs until space runs out
   (reserving 3 cols for "…").
4. **"…" overflow**: Always on far right if tabs don't all fit. Clickable →
   opens folder picker.
5. **Right-aligned counts**: Unread/total count on the right, same as current.

### Tab Styling

- Selected: bold white on blue
- Unselected maildir folders (`/`): white on dark gray
- Unselected splits (`#`): cyan on dark gray
- Unselected smart folders (`@`): yellow on dark gray
- Prefixes kept in display text

### Scroll Adjustment

When selected folder changes:
1. Find index in `tabs`. If not found, highlight nothing.
2. Inbox (index 0) is pinned — selecting it requires no scroll.
3. For other tabs: ensure selected index is within visible window with at
   least one neighbor on each side when possible.

## Mouse Hit Testing

### TabRegion

```rust
pub struct TabRegion {
    pub x_start: u16,
    pub x_end: u16,
    pub kind: TabRegionKind,
}

pub enum TabRegionKind {
    Account,
    Tab(usize),    // index into app.tabs
    Overflow,      // the "…" button
}
```

Regions recomputed every frame during render (cheap, handful of entries).

### Click Handling

- **Account** → open account picker popup
- **Tab(i)** → `navigate_folder(&tabs[i])`
- **Overflow** → open folder picker (`InputMode::FolderPicker`)

## Account Picker

Small dropdown popup anchored below the account badge. Shows all account
names. New `InputMode::AccountPicker`.

- Click account name → switch account
- Arrow keys + Enter work too
- Escape or click-outside dismisses
- Keyboard shortcut: `gA`

## Keyboard

- **Tab / Shift-Tab**: Cycle through `tabs` list (wraps around), navigating
  to each folder. Replaces current known_folders cycling.
- **gA**: Open account picker popup.
- All existing folder shortcuts (`gi`, `ga`, `gd`, etc.) still work and
  update tab highlight if the target folder is in tabs.

## Thread View

When in thread view, the tab bar shows the thread subject (current behavior)
instead of tabs. Tabs return when exiting thread view.
