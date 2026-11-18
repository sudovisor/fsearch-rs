//! Port of `fsearch_file_utils_cmp_paths` (`src/fsearch_file_utils.c:788`): glibc `strverscmp`
//! with `/` sorting directly above the terminating NUL.

use std::cmp::Ordering;

const S_N: usize = 0x0;
const S_I: usize = 0x3;
const S_F: usize = 0x6;
const S_Z: usize = 0x9;
const CMP: i8 = 2;
const LEN: i8 = 3;

const NEXT_STATE: [usize; 12] = [S_N, S_I, S_Z, S_N, S_I, S_I, S_N, S_F, S_F, S_N, S_F, S_Z];

#[rustfmt::skip]
const RESULT_TYPE: [i8; 36] = [
    CMP, CMP, CMP, CMP, LEN, CMP, CMP, CMP, CMP,
    CMP, -1,  -1,  1,   LEN, LEN, 1,   LEN, LEN,
    CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP,
    CMP, 1,   1,   -1,  CMP, CMP, -1,  CMP, CMP,
];

#[inline(always)]
fn weight(c: u8) -> i32 {
    match c {
        0 => 0,
        b'/' => 1,
        c => c as i32 + 1,
    }
}

#[inline(always)]
fn at(s: &[u8], i: usize) -> u8 {
    // C strings: reading at the end gives the NUL terminator.
    if i < s.len() { s[i] } else { 0 }
}

#[inline(always)]
fn class(c: u8) -> usize {
    (c == b'0') as usize + c.is_ascii_digit() as usize
}

/// Natural ("version") comparison of two names, returning <0, 0 or >0 like the C function.
pub fn cmp_paths(s1: &[u8], s2: &[u8]) -> i32 {
    // Fast path: plain bytes differ before any digit is involved.
    let mut p1 = 0usize;
    let mut p2 = 0usize;
    let mut c1 = at(s1, p1);
    let mut c2 = at(s2, p2);
    p1 += 1;
    p2 += 1;
    let mut state = S_N + class(c1);
    let mut diff;
    loop {
        diff = weight(c1) - weight(c2);
        if diff != 0 {
            break;
        }
        if c1 == 0 {
            return 0;
        }
        state = NEXT_STATE[state];
        c1 = at(s1, p1);
        c2 = at(s2, p2);
        p1 += 1;
        p2 += 1;
        state += class(c1);
    }
    let rt = RESULT_TYPE[state * 3 + class(c2)];
    match rt {
        CMP => diff,
        LEN => {
            loop {
                let d1 = at(s1, p1);
                p1 += 1;
                if !d1.is_ascii_digit() {
                    break;
                }
                let d2 = at(s2, p2);
                p2 += 1;
                if !d2.is_ascii_digit() {
                    return 1;
                }
            }
            if at(s2, p2).is_ascii_digit() {
                -1
            } else {
                diff
            }
        }
        other => other as i32,
    }
}

#[inline]
pub fn cmp_paths_ord(a: &[u8], b: &[u8]) -> Ordering {
    cmp_paths(a, b).cmp(&0)
}

/// `strcmp` on byte strings, as an `Ordering`.
#[inline]
pub fn strcmp(a: &[u8], b: &[u8]) -> Ordering {
    a.cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: &str, b: &str) -> i32 {
        cmp_paths(a.as_bytes(), b.as_bytes()).signum()
    }

    #[test]
    fn natural() {
        assert_eq!(s("file2", "file10"), -1);
        assert_eq!(s("file10", "file2"), 1);
        assert_eq!(s("file02", "file2"), -1);
        assert_eq!(s("a", "a"), 0);
        assert_eq!(s("a/b", "a.b"), -1);
        assert_eq!(s("", "a"), -1);
        assert_eq!(s("v1.9", "v1.10"), -1);
        assert_eq!(s("Z", "a"), -1);
    }
}
