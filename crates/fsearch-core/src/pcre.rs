//! Regex matching the way the original does it (`fsearch_query_matcher_regex`): one compiled
//! PCRE2 pattern, JIT-compiled, matched with `pcre2_jit_match` and one match-data block per
//! thread. The safe `pcre2` crate goes through `pcre2_match` and a shared pool, which costs more
//! per file name than the match itself for typical patterns.
//!
//! All `unsafe` code of the crate's regex handling is in this module.

use pcre2_sys::*;
use std::ptr;

struct MatchData(*mut pcre2_match_data_8);

// SAFETY: a match-data block is plain memory owned by exactly one thread (it lives in a
// `ThreadLocal` slot and is never shared); moving ownership between threads is fine.
unsafe impl Send for MatchData {}

impl Drop for MatchData {
    fn drop(&mut self) {
        // SAFETY: the pointer came from pcre2_match_data_create_from_pattern_8 and is freed once.
        unsafe { pcre2_match_data_free_8(self.0) }
    }
}

pub struct JitRegex {
    code: *mut pcre2_code_8,
    jit: bool,
    match_data: thread_local::ThreadLocal<MatchData>,
}

// SAFETY: a compiled PCRE2 pattern (and its JIT code) is read-only after compilation; PCRE2
// documents it as safe to use from several threads at once. Mutable per-match state is the
// per-thread match data.
unsafe impl Send for JitRegex {}
// SAFETY: see `Send`; `&JitRegex` only reads `code` and touches the caller thread's match data.
unsafe impl Sync for JitRegex {}

impl JitRegex {
    /// `pcre2_compile` with `PCRE2_UTF` (+ `PCRE2_CASELESS`), then `pcre2_jit_compile`.
    pub fn new(pattern: &[u8], caseless: bool) -> Option<JitRegex> {
        let options = PCRE2_UTF | if caseless { PCRE2_CASELESS } else { 0 };
        let mut error_code = 0;
        let mut error_offset: usize = 0;
        // SAFETY: the pattern pointer and length describe a live byte slice; the out-parameters
        // point to live locals; a null compile context selects the defaults.
        let code = unsafe {
            pcre2_compile_8(
                pattern.as_ptr(),
                pattern.len(),
                options,
                &mut error_code,
                &mut error_offset,
                ptr::null_mut(),
            )
        };
        if code.is_null() {
            return None;
        }
        // SAFETY: `code` is a valid compiled pattern.
        let jit = unsafe { pcre2_jit_compile_8(code, PCRE2_JIT_COMPLETE) } == 0;
        Some(JitRegex {
            code,
            jit,
            match_data: thread_local::ThreadLocal::new(),
        })
    }

    pub fn is_match(&self, subject: &[u8]) -> bool {
        // The original passes names unchecked to pcre2_jit_match (undefined for invalid UTF-8);
        // here an invalid name simply does not match, as with pcre2_match.
        if self.jit && std::str::from_utf8(subject).is_err() {
            return false;
        }
        let md = self.match_data.get_or(|| {
            // SAFETY: `self.code` is valid for the lifetime of `self`; a null context = defaults.
            MatchData(unsafe { pcre2_match_data_create_from_pattern_8(self.code, ptr::null_mut()) })
        });
        if md.0.is_null() {
            return false;
        }
        static EMPTY: [u8; 1] = [0];
        let ptr = if subject.is_empty() {
            EMPTY.as_ptr()
        } else {
            subject.as_ptr()
        };
        // SAFETY: `code` and the thread's own match data are valid; `ptr`/`len` describe a live
        // slice (or a valid one-byte buffer with length 0); the subject is valid UTF-8 when the
        // JIT path skips PCRE2's own check.
        let rc = unsafe {
            if self.jit {
                pcre2_jit_match_8(self.code, ptr, subject.len(), 0, 0, md.0, ptr::null_mut())
            } else {
                pcre2_match_8(self.code, ptr, subject.len(), 0, 0, md.0, ptr::null_mut())
            }
        };
        rc > 0
    }
}

impl Drop for JitRegex {
    fn drop(&mut self) {
        // match data first, then the pattern it was created from
        self.match_data.clear();
        // SAFETY: `code` came from pcre2_compile_8 and is freed exactly once.
        unsafe { pcre2_code_free_8(self.code) }
    }
}

#[cfg(test)]
mod tests {
    use super::JitRegex;

    #[test]
    fn matches_like_pcre2() {
        let r = JitRegex::new(b"^lib.*\\.so$", false).expect("compile");
        assert!(r.is_match(b"libc.so"));
        assert!(!r.is_match(b"libc.so.6"));
        assert!(!r.is_match(b"\xff"));
        let c = JitRegex::new(b"report", true).expect("compile");
        assert!(c.is_match(b"Final REPORT.pdf"));
        assert!(!c.is_match(b""));
        assert!(JitRegex::new(b"(", false).is_none());
    }
}
