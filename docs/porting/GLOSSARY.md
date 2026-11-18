# Glossary
| Original term | Meaning | Rust name |
| --- | --- | --- |
| FsearchDatabase | Indexed set of files/folders plus search state | `fsearch_core::Store` (+ `gui::backend` worker) |
| FsearchDatabaseEntry | One file or folder node (name, parent, size, mtime) | `fsearch_core::db::Entry`, referenced by `db::Id` |
| Index store | All indexes plus the fast-sort arrays | `fsearch_core::Store`, `store::Sorted` |
| Include / Exclude | Roots to scan / paths and patterns to skip | `fsearch_core::rules::{Include, Exclude, ExcludeManager}` |
| Query | Parsed search expression | `fsearch_core::query::Query` (tree: `query::Tree`) |
| Query node | One matcher or operator in the query tree | `query::Leaf` / `query::Matcher` |
| Match data | Per-thread scratch for matching one entry | `query::MatchData` |
| Filter / macro | Named preset query; its macro is usable as `macro:` | `fsearch_core::filter::Filter` |
| Search view | Sorted, filtered result list shown in the window | `fsearch_core::View` |
| Sort order chain | Properties compared in turn when sorting | `Vec<db::Prop>` (`store::chain_for`) |
| Fast sort index | Precomputed order of all entries for one property | `store::Sorted` in `Store::sorted[prop]` |
| cmp_paths | Natural (version) name comparison | `fsearch_core::cmp::cmp_paths` |
