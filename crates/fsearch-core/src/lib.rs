//! Pure Rust core of FSearch: index, scan, query, sort and the `fsearch.db` / `fsearch.conf`
//! formats. Ported from the C original (`src/`), which stays in the repository as reference.

pub mod cmp;
pub mod config;
pub mod content;
pub mod db;
pub mod file;
pub mod filter;
pub mod mime;
pub mod pcre;
pub mod query;
pub mod rules;
pub mod scan;
pub mod size;
pub mod store;
pub mod time;
pub mod utf;

pub use db::{Id, Prop};
pub use query::Query;
pub use store::{SortDir, Store, View};
