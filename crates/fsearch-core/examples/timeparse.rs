//! Prints `input<TAB>start<TAB>end` (or `-`) for each line on stdin (compare with the C parser).
use std::io::BufRead;
fn main() {
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        match fsearch_core::time::parse_interval(line.as_bytes()) {
            Some((a, b)) => println!("{line}\t{a}\t{b}"),
            None => println!("{line}\t-"),
        }
    }
}
