# Changelog

## 0.3.0

- New interface: a search field with keycap toggles for match case, regular expressions and search in path,
  responsive columns, empty states that explain what to do next, and a status bar with result count and time.
- Eight looks in Preferences → Appearance: Modern (dark, light), Ubuntu (light, dark), macOS (light, dark),
  Windows 95 and Classic FSearch. The look previews live and Cancel restores the previous one.
- Faster: parallel sorting by Type (1.4 s → 3 ms on 876,000 entries), search as you type filters the previous
  results when the query narrows (3.6× per keystroke), cached folder paths for path searches, direct PCRE2 JIT
  matching, binary search for extension lists, an ASCII fast path for case-insensitive Unicode search.
- Windows 10/11 build with an installer and a portable zip.
- Fixes: a damaged `fsearch.db` no longer allocates unbounded memory or loops on a folder cycle; very long or
  deeply nested queries no longer overflow the stack; an unreadable `fsearch.conf` is never overwritten;
  `Ctrl+Shift+R` updates the index instead of toggling regex; `Ctrl+A` and `Ctrl+C` act on the text while
  the search field has focus; opening more than 10 files asks first; the single-instance socket moved out of
  shared `/tmp` and only accepts the same user; rescans triggered by folder monitoring are throttled;
  Chinese, Japanese and Korean file names render with system fallback fonts.

## 0.2.0

- Pure Rust interface with egui, replacing GTK.

## 0.1.0

- Rust port of FSearch with identical search results, sort orders, database and configuration formats.
