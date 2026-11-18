# Port map: fsearch → Rust

Goal: a pure Rust (egui, ADR-010) fsearch, shipped as an AppImage, with measured speed against the C original.
Decisions: see [DECISIONS.md](DECISIONS.md) — ADR-001 shape, 002 toolkit, 003 scope, 004 formats, 005 regex/ICU, 006 equivalence, 007 bench, 008 AppImage, 009 checkpoints, 010 egui (supersedes 002), 011 stderr, 012 core as one crate, 013 monitoring, 014 driver subcommands, 015 known differences, 016 AppImage/glibc, 017 regex FFI, 018 optimizations, 019 redesign and looks, 020 hardening, 021 UI tests, 022 Windows.
Plan: [../../plan.md](../../plan.md) (`rust-anything analyze --whole-app`).

## Units (wave 1, core)
| Unit | State |
| --- | --- |
| string_utils | done 2026-11-18: `rust/fsearch-string-utils-rs`, gate passed, 1035/1035 recorded cases |
| utf, size_utils, time_utils | done 2026-11-18: in `fsearch-core` (ADR-012); C test tables ported, dates compared against C |
| query lexer + parser + node + tree + matchers + match_data + query | done 2026-11-18: `fsearch-core::query`; 178 C test rows + Turkish table; 5,000-query fuzz, 0 mismatches |
| array, chunked_array, entry, entry_info, sort | done 2026-11-18: `fsearch-core::{db,store}` |
| include/exclude (+ managers), filter, filter_manager | done 2026-11-18: `fsearch-core::{rules,filter}` |
| database scan, index, index_store, file, work, search_view, database | done 2026-11-18: `fsearch-core::{scan,store,file}`; `fsearch.db` byte-identical |
| config | done 2026-11-18: `fsearch-core::config`; same `fsearch.conf` layout |
| fsearch.c, window, window_actions, result_view, list_view, statusbar, popup, clipboard, preview, main | done 2026-11-18: `rust/app/src/gui` (egui) |

## Units (wave 2)
| Unit | State |
| --- | --- |
| preferences_dialog, database_preferences_widget, filter_preferences_widget, filter_editor | done 2026-11-18: `gui/prefs.rs` |
| folder_monitor_inotify, folder_monitor_fanotify, folder_monitor_event, rescan_manager | done 2026-11-18: `gui/monitor.rs` (ADR-013) |

## State
- Lifted: `rust/app` is a pure Rust app (no fallback, no kill switch).
- `app replay`: 400/400, fallback rate 0% (`rust/app-cases/replay-report.md`), also via the AppImage.
- AppImage: `rust/packaging/build-appimage.sh`. Speed: [BENCHMARK.md](BENCHMARK.md).
- Handoff: [HANDOFF.md](HANDOFF.md). Cutover (user steps): `cutover.sh`.
- Known differences: ADR-015.

## Open questions
- None.

## Out of scope
- Translations beyond loading existing .po catalogs; snap/flatpak/debian packaging.
