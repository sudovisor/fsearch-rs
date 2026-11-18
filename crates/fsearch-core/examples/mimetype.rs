//! Prints `name<TAB>mime<TAB>description` for each name on stdin (compare with GIO).
use std::io::BufRead;
fn main() {
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let m = fsearch_core::mime::guess(line.as_bytes());
        println!("{}\t{}\t{}", line, m, fsearch_core::mime::description(&m));
    }
}
