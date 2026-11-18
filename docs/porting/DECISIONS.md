# Decisions: fsearch → Rust

## ADR-001: Desktop GUI app, not a CLI
Status: accepted (2026-11-18)
Context: `analyze --whole-app` labelled the app `cli` because of `main.c`; fsearch is a GTK3 GApplication with a few command-line flags.
Decision: Port as a desktop GUI app. The CLI surface (`-s/--search`, `-u/--update-database`, `-v/--version`, `--new-window`, `--minimized`, `--preferences`) is kept.
Consequences: `app record`/`replay` cover only the CLI flags and a headless query driver; UI equivalence is checked by hand.

## ADR-002: UI toolkit is gtk4-rs
Status: superseded by ADR-010 (2026-11-18)
Context: gtk3-rs is unmaintained; a pure-Rust toolkit would look less native.
Decision: gtk4-rs (GTK 4). Layout and actions follow the GTK3 original.
Consequences: Custom list view (`fsearch_list_view.c`) is rewritten on `gtk::ColumnView` with a virtual list model instead of being translated line by line.

## ADR-003: Scope in two waves
Status: accepted (2026-11-18)
Decision: Wave 1 (core): indexer, include/exclude, query syntax, matchers, sort, main window and list, open/copy actions, CLI flags. Wave 2: preferences dialog, filter editor, folder monitoring (inotify/fanotify). Both waves land before cutover.

## ADR-004: Byte-compatible config and database
Status: accepted (2026-11-18)
Decision: Read and write the same `fsearch.conf` (GKeyFile) and `fsearch.db` binary formats at the same paths.
Consequences: Existing users' settings and index carry over; DB round-trip is a recorded test.

## ADR-005: pcre2 crate and ICU4X
Status: accepted (2026-11-18)
Decision: Regex via the `pcre2` crate (vendored), case folding/normalization via ICU4X (`icu_casemap`, `icu_normalizer`).
Consequences: Regex semantics match the original; Unicode case folding follows ICU data (version may differ slightly from system ICU — recorded traps decide).

## ADR-006: Equivalence bar
Status: accepted (2026-11-18)
Decision: For each recorded query over fixed fixture trees, the result set and order must match exactly. The original's 12 C test suites become characterization cases. UI is checked manually.

## ADR-007: Benchmark for time savings
Status: accepted (2026-11-18)
Decision: Measure index build (`/usr` + `$HOME`), DB load and save, query latency (plain, wildcard, regex), re-sort, cold start to first window. Several runs each, report medians, original vs Rust.

## ADR-008: Packaging is an x86_64 AppImage
Status: amended by ADR-010 (2026-11-18)
Decision: Build with linuxdeploy + linuxdeploy-plugin-gtk + appimagetool (downloaded from their GitHub releases), GTK 4 bundled.

## ADR-009: Checkpoints and cutover
Status: accepted (2026-11-18)
Decision: Stop for the user's go-ahead after each wave. The original stays in place beside the Rust app until the user retires it.

## ADR-010: UI toolkit is egui (pure Rust), superseding ADR-002
Status: accepted (2026-11-18)
Context: The user chose a pure-Rust toolkit over gtk4-rs. Between egui and iced, egui (eframe) fits best: immediate-mode lists with row virtualization (`egui_extras::TableBuilder` renders only visible rows, needed for millions of results), built-in menus, dialogs and keyboard handling, and no system GUI libraries at build time.
Decision: The UI is written in egui/eframe (glow or wgpu backend, X11 and Wayland). Layout and actions follow the GTK3 original: search entry, filter selector, results table (Name, Path, Size, Type, Date Modified) with column sorting, status bar, context menu, preferences window.
Consequences: The Rust build needs no GTK, GLib or Pango; `libgtk-4-dev` is not needed. The look is egui's own, not native GTK: no GTK theme, no system file-chooser (`rfd` with its xdg-portal backend for folder pickers). The AppImage bundles only the binary plus desktop file and icon; it relies on the system's libGL/EGL, X11/Wayland libs (the AppImage norm), so linuxdeploy's GTK plugin is no longer needed (amends ADR-008). Opening files uses `xdg-open`/the `open` crate in place of GIO.

## ADR-011: GLib diagnostic logs on stderr are not part of the contract
Status: accepted (2026-11-18)
Context: The original prints GLib log lines (`(process:PID): fsearch-…-WARNING **: HH:MM:SS.mmm: …`) on stderr for warnings and failed assertions; they carry PIDs and timestamps and are diagnostics, not output. ADR-006 sets the bar at the result set and its order.
Decision: Recorded cases whose original wrote to stderr expect `...` (anything) on stderr; stdout and exit code must match exactly.
Consequences: The port may log differently (or not at all) on the same error paths. The headless driver loads each case's database from a private copy, because the original takes an exclusive `flock` on the database even for reading and cases run in parallel; the Rust app keeps that lock.

## ADR-012: The tangled core is ported as one crate, `fsearch-core`
Status: accepted (2026-11-18)
Context: `rust-anything bridge` gives every C unit a C-ABI drop-in, but it can only move functions with plain types: every export of the database, index store, query and scan units passes GObject/GLib pointers and "must stay in C". The skill allows a tangled core to move in one piece.
Decision: `fsearch_string_utils.c` goes through the full unit loop (bridge, record, port, diff, bench, gate). Everything else non-UI (query lexer/parser/nodes/matchers, entries, index store, sort, scan, include/exclude, filters, size/time/utf helpers, `fsearch.db` and `fsearch.conf` formats) is `rust/fsearch-core`, a pure Rust crate built on the string-utils unit. Its proof is app level: the 400 recorded driver cases (`rust/app-cases`), a differential fuzz of random queries against the C driver (`rust/harness/fuzz_queries.py`), the original's C test tables ported to `fsearch-core/tests`, an exhaustive MIME-description comparison against GIO and a date-parser comparison against the C function. The front door routes the driver commands through the gated unit's glue (`rust/app/src/units/fsearch_string_utils.rs`).
Consequences: `gate` covers the unit crate; the core is held to `cargo fmt`, `clippy -D warnings`, its tests and `app replay` (100%).

## ADR-013: Folder monitoring rescans instead of patching the index
Status: accepted (2026-11-18)
Context: The original applies inotify/fanotify events entry by entry (`fsearch_folder_monitor_*.c`, `fsearch_database_rescan_manager.c`), about 1,300 lines of GLib-thread code. The parallel Rust scanner rescans `/usr` (≈350k entries) in about 0.2 s.
Decision: Includes marked "monitor" are watched recursively with `notify` (inotify); a change triggers a debounced (1.5 s) rescan of the database. `rescan_after` triggers periodic rescans; `scan_after_launch` rescans after loading. The database and config folders are ignored to avoid feedback loops.
Consequences: Very large monitored trees rescan in full instead of incrementally. fanotify (root-only) is not used.

## ADR-014: Headless driver subcommands in the shipped binary
Status: accepted (2026-11-18)
Context: The recorded cases drive a headless CLI (`dump`, `query`, `resave`, `scandump`, `scanmd5`) that the C original lacks; `rust/harness/fsdriver.c` adds it to the original's core library.
Decision: The Rust `fsearch` binary answers the same subcommands, so `cargo test`/`app replay` can run the cases against the shipped binary. Without a subcommand it behaves like the original: GApplication-style options (`-s`, `-u`, `-v`, `--minimized`, `--preferences`, `--new-window`), single instance (a second launch forwards its options to the first), then the window.
Consequences: `fsearch query …` etc. are extra, undocumented entry points. `FSEARCH_SCREENSHOT=<png>` is a test hook that saves one frame and exits.

## ADR-015: Known behavior differences
Status: accepted (2026-11-18)
- `contenttype:` uses shared-mime-info globs plus a small sniff (empty, text, binary) instead of GIO's full magic sniffing.
- Highlighting: the port also highlights Unicode (non-ASCII) matches as matched rows; the original highlights only ASCII and regex matches.
- An ambiguous wall time (DST fall-back hour) parsed in `dm:` may resolve to the other offset than the C build, depending on earlier `mktime` calls in the process (glibc caches the offset).
- Multiple windows (`--new-window`) open one window; the original can open several.
- GLib warnings on stderr are not reproduced (ADR-011).

## ADR-016: AppImage contents and glibc floor (amends ADR-008)
Status: accepted (2026-11-18)
Context: egui loads X11/Wayland/OpenGL at run time, so the binary links only glibc, libm and libgcc. Built on glibc 2.43, it picked up `atan2f@GLIBC_2.43`, `hypot(f)@GLIBC_2.35` and `fmod@GLIBC_2.38`.
Decision: PCRE2 is linked statically (`PCRE2_SYS_STATIC=1`). `rust/app/compat/glibc_compat.c` binds those four libm functions to their `GLIBC_2.2.5` versions through `--wrap`. The AppImage (`rust/packaging/build-appimage.sh`) holds the binary, the desktop file, the SVG icon and the man page, packed with appimagetool and the type-2 runtime (no linuxdeploy, no bundled libraries).
Consequences: Runs on x86_64 Linux with glibc ≥ 2.34 (Ubuntu 22.04+, Debian 12+, Fedora 35+) and an X11 or Wayland session with OpenGL. The AppImage is about 5 MB.

## ADR-017: Regex matching through `pcre2_jit_match` (FFI)
Status: accepted (2026-11-18)
Context: The safe `pcre2` crate matches through `pcre2_match` and a shared match-data pool; per file name that cost more than the match and made regex and wildcard searches slower than C.
Decision: `fsearch-core/src/pcre.rs` calls `pcre2_jit_match` with one match-data block per thread, as the original does; it is the crate's only regex `unsafe` code, with `SAFETY` comments. Invalid UTF-8 names do not match (the original's JIT path leaves that undefined). Wildcards made only of ASCII are matched directly on ASCII names without newlines (same answer as the generated `^…$` regex).
Consequences: Regex searches run at parity with C (same engine); wildcard searches are faster.

## ADR-018: Optimizations beyond the original's algorithms
Status: accepted (2026-11-18)
Context: The port reproduced the original's algorithms; several repeat work the original never avoided.
Decision:
- Search as you type filters the previous results when the new query can only narrow the old one (`Query::refines`: same filter, every old AND-term implied by a new substring term). Results are identical to a full search (checked on 1,160 simulated keystrokes), 3.6× faster per keystroke.
- Path searches build paths from a once-per-index cache of folder paths (`db::FolderPaths`) instead of walking the parent chain per entry.
- Unicode case folding reuses buffers; sorted arrays and views are shared (`Arc`), so match-all and re-sorting by a fast-sort property copy nothing.
- Sort by Type is a stable sort by type rank of a list already in the previous order, which equals the original's comparator chain `[type, previous chain…]` (1.3 s in C, 2.6 ms here).
- AND/OR are flat lists in the query tree.
Consequences: Equivalence unchanged (400/400 replay, 17,000+ fuzzed queries).

## ADR-019: Redesigned window with selectable looks (supersedes the look part of ADR-010)
Status: accepted (2026-11-18)
Context: The first egui window used stock egui styling; the user asked for a modern, responsive redesign without losing access to tools and shortcuts, plus looks inspired by Ubuntu 26.04, Windows 95 and macOS, and the classic look.
Decision: Direction: "fast, exact, quietly technical"; world: a card catalog with highlighter marks. Default look Modern (graphite neutrals, one amber accent for matches and focus). Signature: keycaps — search toggles `Aa` `.*` `/` live in the search field, every shortcut is shown as a keycap (menus, tooltips, Preferences → Shortcuts). Looks: Modern, Modern light, Ubuntu, Ubuntu dark, macOS, macOS dark, Windows 95, Classic (`gui/theme.rs`), chosen in Preferences → Appearance (live previews) or View → Look; stored as `look=` in `[Interface]` (the C build ignores the key). Responsive: secondary columns step aside on narrow windows, paths are elided from the left, the toolbar reflows. Empty states for no folders, loading, no results. Every custom widget exposes an accessibility role and label.
Consequences: Menus, shortcuts and context menu keep every original command; the header bar looks (Ubuntu, macOS) reach them through a menu button.

## ADR-020: Hardening found by the bug hunt
Status: accepted (2026-11-18)
- `fsearch.db`: counts are checked against the block sizes before allocating (a corrupt file aborted with an 84 GB allocation), folder parents must reach a root (a cycle hung path building), names longer than 65,535 bytes are rejected. Fuzzed with 6,500 mutated databases, 0 panics.
- Queries: AND/OR chains are flat, the tree drops iteratively and nesting is capped (tree depth 1024, parser depth 256), so pasted megabyte queries and deep `case:(…` nesting no longer overflow the stack; the lexer shares its input (lookahead was quadratic).
- Single instance: never in shared `/tmp`; socket in `$XDG_RUNTIME_DIR` or `~/.cache/fsearch` (0700), a socket owned by another user is neither used nor removed, peers are checked with `SO_PEERCRED`, messages are capped at 64 KiB. Windows: localhost port plus a per-session token.
- Config: read even with stray non-UTF-8 bytes, written atomically (temp file + rename), never overwritten with defaults when it cannot be read.
- Monitoring: automatic rescans are coalesced and spaced by four times the last scan's duration.
- UI: Ctrl+Shift+R updated nothing and toggled regex (egui matched Ctrl+R first); Ctrl+A in the search field selected all rows; cancelling Preferences kept a previewed look; opening more than ten items now asks first.
- Fonts: CJK and other scripts render (wide-coverage system fonts are loaded in the background).

## ADR-021: Tests for every window function
Status: accepted (2026-11-18)
Decision: `src/gui/tests.rs` drives the real window headlessly through the accessibility tree (egui_kittest): typing, toggles, shortcuts, update, filter, sorting, selection and keyboard navigation, empty states, preferences and every look, trash (into the test's own trash), the mass-open guard, menus and About, a narrow window. Each scenario has its own config, data, runtime directories and generated fixture.

## ADR-022: Windows build
Status: accepted (2026-11-18)
Context: The user asked for a Windows installation; the port used Unix-only APIs (rustix scanning and locking, libc time functions, Unix sockets, raw byte paths, XDG directories).
Decision: Cross-compiled with `x86_64-pc-windows-gnu` (MinGW). Scanner on `std::fs`; database lock via `File::try_lock` everywhere; dates through chrono (tested against libc on 2,600 cases); file types from a built-in table with shared-mime-info's descriptions; settings in `%APPDATA%\fsearch`, the index in `%LOCALAPPDATA%\fsearch`; paths are stored and shown with `/` (Windows accepts it, and `\` is the query escape character), converted to `\` when opening. GUI subsystem with the icon and version info embedded; command-line use attaches to the parent console. Installer built with NSIS, plus a portable zip.
Consequences: Compiled and packaged here but not run on Windows (no Windows or Wine on the build machine). `one_file_system` and `contenttype:` sniffing are limited on Windows.
