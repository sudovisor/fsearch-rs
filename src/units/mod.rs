//! Written by `rust-anything app lift`: every command is answered in Rust.

use std::process::ExitCode;

use clap::ArgMatches;

mod fsearch_string_utils;

pub fn dispatch(command: &str, matches: &ArgMatches) -> Option<ExitCode> {
    match command {
        "dump" | "query" | "resave" | "scandump" | "scanmd5" | "fsearch" => {
            Some(fsearch_string_utils::run(command, matches))
        }
        _ => None,
    }
}
