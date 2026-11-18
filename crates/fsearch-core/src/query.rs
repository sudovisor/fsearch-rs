//! Port of the query engine: `fsearch_query_lexer.c`, `fsearch_query_parser.c`,
//! `fsearch_query_node.c`, `fsearch_query_tree.c`, `fsearch_query_matchers.c`,
//! `fsearch_query_match_data.c` and `fsearch_query.c`.
//!
//! The parser keeps the original's structure (an operator-precedence parser with one shared
//! context, implicit AND, filter macros with their own lexer) because its corner cases are part of
//! the recorded behavior.

use crate::db::{Db, Id};
use crate::filter::Filter;
use crate::{size, time, utf};
use fsearch_string_utils_rs::port as su;
use memchr::memmem;

pub mod flags {
    pub const MATCH_CASE: u32 = 1 << 0;
    pub const AUTO_MATCH_CASE: u32 = 1 << 1;
    pub const REGEX: u32 = 1 << 2;
    pub const SEARCH_IN_PATH: u32 = 1 << 3;
    pub const AUTO_SEARCH_IN_PATH: u32 = 1 << 4;
    pub const FILES_ONLY: u32 = 1 << 5;
    pub const FOLDERS_ONLY: u32 = 1 << 6;
    pub const EXACT_MATCH: u32 = 1 << 7;
}
use flags::*;

// ---------------------------------------------------------------------------------------------
// Lexer

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Token {
    None,
    Eos,
    Word,
    Field,
    FieldEmpty,
    Equal,
    Contains,
    Smaller,
    SmallerEq,
    Greater,
    GreaterEq,
    Not,
    And,
    Or,
    BracketOpen,
    BracketClose,
}

#[derive(Clone)]
struct Lexer {
    /// shared, so the parser's lookahead (a clone of the lexer) does not copy the query
    input: std::sync::Arc<[u8]>,
    pos: usize,
    stack: Vec<u8>,
}

#[inline]
fn g_ascii_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// A GString used as a C string afterwards: everything after an embedded NUL is invisible.
fn c_str(mut v: Vec<u8>) -> Vec<u8> {
    if let Some(i) = v.iter().position(|&b| b == 0) {
        v.truncate(i);
    }
    v
}

impl Lexer {
    fn new(input: &[u8]) -> Lexer {
        Lexer {
            input: std::sync::Arc::from(input),
            pos: 0,
            stack: Vec::new(),
        }
    }

    fn next_char(&mut self) -> u8 {
        if let Some(c) = self.stack.pop() {
            return c;
        }
        if self.pos < self.input.len() {
            let c = self.input[self.pos];
            self.pos += 1;
            c
        } else {
            0
        }
    }

    fn give_back(&mut self, c: u8) {
        self.stack.push(c);
    }

    fn parse_quoted(&mut self, out: &mut Vec<u8>) {
        loop {
            let c = self.next_char();
            if c == 0 || c == b'"' {
                return;
            }
            out.push(c);
        }
    }

    fn next_token(&mut self) -> (Token, Vec<u8>) {
        let mut c;
        loop {
            c = self.next_char();
            if c == 0 || !g_ascii_isspace(c) {
                break;
            }
        }
        match c {
            0 => return (Token::Eos, Vec::new()),
            b'=' => return (Token::Equal, Vec::new()),
            b':' => return (Token::Contains, Vec::new()),
            b'<' => {
                let c1 = self.next_char();
                if c1 == b'=' {
                    return (Token::SmallerEq, Vec::new());
                }
                self.give_back(c1);
                return (Token::Smaller, Vec::new());
            }
            b'>' => {
                let c1 = self.next_char();
                if c1 == b'=' {
                    return (Token::GreaterEq, Vec::new());
                }
                self.give_back(c1);
                return (Token::Greater, Vec::new());
            }
            b'!' => return (Token::Not, Vec::new()),
            b'(' => return (Token::BracketOpen, Vec::new()),
            b')' => return (Token::BracketClose, Vec::new()),
            _ => {}
        }
        self.give_back(c);
        let mut value: Vec<u8> = Vec::new();
        loop {
            let c = self.next_char();
            if c == 0 {
                break;
            }
            if g_ascii_isspace(c) {
                break;
            } else if c == b'"' {
                self.parse_quoted(&mut value);
            } else if c == b'\\' {
                let e = self.next_char();
                value.push(e);
            } else if b":=<>()".contains(&c) {
                if c == b':' {
                    let n = self.next_char();
                    let tok = if g_ascii_isspace(n) || n == 0 {
                        Token::FieldEmpty
                    } else {
                        self.give_back(n);
                        Token::Field
                    };
                    return (tok, c_str(value));
                }
                self.give_back(c);
                break;
            } else {
                value.push(c);
            }
        }
        let value = c_str(value);
        match value.as_slice() {
            b"NOT" => (Token::Not, Vec::new()),
            b"AND" | b"&&" => (Token::And, Vec::new()),
            b"OR" | b"||" => (Token::Or, Vec::new()),
            _ => (Token::Word, value),
        }
    }

    fn peek(&self) -> Token {
        self.clone().next_token().0
    }
}

// ---------------------------------------------------------------------------------------------
// Nodes

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmp {
    Equal,
    Greater,
    GreaterEq,
    Smaller,
    SmallerEq,
    Range,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumProp {
    DateModified,
    Size,
    Depth,
    ChildCount,
    ChildFileCount,
    ChildFolderCount,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hay {
    Name,
    Path,
    ParentPath,
    ContentType,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsciiOp {
    StrStr,
    StrCaseStr,
    StrCmp,
    StrCaseCmp,
}

/// A compiled regex with one clone per search thread: clones share the JIT code but each has its
/// own match-data pool, like the original's per-thread `pcre2_match_data`.
pub struct Re {
    base: pcre2::bytes::Regex,
    per_thread: thread_local::ThreadLocal<pcre2::bytes::Regex>,
    /// Matching uses the original's `pcre2_jit_match` path (`crate::pcre`).
    jit: crate::pcre::JitRegex,
}

impl Re {
    fn new(base: pcre2::bytes::Regex, jit: crate::pcre::JitRegex) -> Re {
        Re {
            base,
            per_thread: thread_local::ThreadLocal::new(),
            jit,
        }
    }

    #[inline]
    pub fn is_match(&self, subject: &[u8]) -> bool {
        self.jit.is_match(subject)
    }

    #[inline]
    pub fn get(&self) -> &pcre2::bytes::Regex {
        self.per_thread.get_or(|| self.base.clone())
    }
}

#[allow(clippy::large_enum_variant)] // one matcher per query term; boxing buys nothing
pub enum Matcher {
    False,
    True,
    Regex {
        re: Re,
        hay: Hay,
        /// For a wildcard with an ASCII pattern: the pattern, matched directly on ASCII subjects
        /// without newlines (where it means exactly what the generated `^…$` regex means).
        glob: Option<(Vec<u8>, bool)>,
    },
    Ascii {
        needle: Vec<u8>,
        finder: memmem::Finder<'static>,
        op: AsciiOp,
        hay: Hay,
    },
    Utf {
        needle: String,
        needle_ascii: bool,
        exact: bool,
        hay: Hay,
    },
    Ext {
        /// Sorted like the original's list; matching is membership (its early exit relies on the
        /// sort order), so a set answers the same. Lowercased when `case` is false.
        terms: Vec<Vec<u8>>,
        set: Vec<Vec<u8>>,
        case: bool,
    },
    Num {
        prop: NumProp,
        cmp: Cmp,
        start: i64,
        end: i64,
    },
}

pub struct Leaf {
    pub flags: u32,
    pub matcher: Matcher,
    pub triggers_auto_match_case: bool,
    pub triggers_auto_match_path: bool,
    pub wants_single_threaded_search: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpKind {
    And,
    Or,
    Not,
}

enum PNode {
    Op(OpKind),
    Leaf(Box<Leaf>),
}

pub enum Tree {
    /// AND/OR chains are flat lists (children in query order), so a long query is wide, not
    /// deep: evaluation and dropping never recurse once per term.
    And(Vec<Tree>),
    Or(Vec<Tree>),
    Not(Box<Tree>),
    Leaf(Box<Leaf>),
}

impl Drop for Tree {
    /// Iterative drop: a deeply nested tree must not overflow the stack.
    fn drop(&mut self) {
        let mut stack: Vec<Tree> = Vec::new();
        let take = |t: &mut Tree, stack: &mut Vec<Tree>| match t {
            Tree::And(v) | Tree::Or(v) => stack.append(v),
            Tree::Not(b) => stack.push(std::mem::replace(&mut **b, Tree::And(Vec::new()))),
            Tree::Leaf(_) => {}
        };
        take(self, &mut stack);
        while let Some(mut t) = stack.pop() {
            take(&mut t, &mut stack);
        }
    }
}

/// Deeper trees (only reachable through thousands of nested brackets or modifiers) match nothing
/// instead of risking a stack overflow during evaluation.
const MAX_TREE_DEPTH: usize = 1024;
/// Nested sub-expressions (`case:(…)`, macros) the parser descends into.
const MAX_PARSE_DEPTH: usize = 256;

fn leaf(flags: u32, matcher: Matcher) -> Leaf {
    Leaf {
        flags,
        matcher,
        triggers_auto_match_case: false,
        triggers_auto_match_path: false,
        wants_single_threaded_search: false,
    }
}

fn match_nothing() -> Leaf {
    leaf(0, Matcher::False)
}

fn match_everything(flags: u32) -> Leaf {
    leaf(flags, Matcher::True)
}

fn new_numeric(prop: NumProp, flags: u32, start: i64, end: i64, mut cmp: Cmp) -> Leaf {
    if prop == NumProp::DateModified && cmp == Cmp::Equal {
        cmp = Cmp::Range;
    }
    leaf(
        flags,
        Matcher::Num {
            prop,
            cmp,
            start,
            end,
        },
    )
}

fn new_regex(term: &[u8], flags: u32) -> Leaf {
    let Ok(pattern) = std::str::from_utf8(term) else {
        return match_nothing();
    };
    let built = pcre2::bytes::RegexBuilder::new()
        .utf(true)
        .caseless(flags & MATCH_CASE == 0)
        .jit_if_available(true)
        .build(pattern);
    let jit = crate::pcre::JitRegex::new(term, flags & MATCH_CASE == 0);
    match (built, jit) {
        (Ok(re), Some(jit)) => leaf(
            flags,
            Matcher::Regex {
                re: Re::new(re, jit),
                glob: None,
                hay: if flags & SEARCH_IN_PATH != 0 {
                    Hay::Path
                } else {
                    Hay::Name
                },
            },
        ),
        _ => match_nothing(),
    }
}

fn new_wildcard(term: &[u8], flags: u32) -> Leaf {
    let mut l = new_regex(&su::convert_wildcard_to_regex(term), flags);
    if let Matcher::Regex { glob, .. } = &mut l.matcher
        && term.is_ascii()
    {
        *glob = Some((term.to_vec(), flags & MATCH_CASE == 0));
    }
    l
}

/// `*`/`?` glob on ASCII bytes, anchored at both ends (`?` is one byte = one ASCII character).
fn glob_ascii(p: &[u8], s: &[u8], caseless: bool) -> bool {
    if !p.contains(&b'?') {
        return glob_segments(p, s, caseless);
    }
    let eq = |a: u8, b: u8| {
        if caseless {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    };
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star_p, mut star_s) = (usize::MAX, 0usize);
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            star_p = pi;
            star_s = si;
            pi += 1;
        } else if pi < p.len() && (p[pi] == b'?' || eq(p[pi], s[si])) {
            pi += 1;
            si += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_s += 1;
            si = star_s;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// A `*`-only glob: literal prefix, literal suffix and the middle pieces in order (leftmost
/// placement of each piece is always optimal).
fn glob_segments(p: &[u8], s: &[u8], caseless: bool) -> bool {
    let eq = |a: &[u8], b: &[u8]| {
        if caseless {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let parts: Vec<&[u8]> = p.split(|&c| c == b'*').collect();
    if parts.len() == 1 {
        return eq(p, s);
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if first.len() + last.len() > s.len()
        || !eq(&s[..first.len()], first)
        || !eq(&s[s.len() - last.len()..], last)
    {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for piece in &parts[1..parts.len() - 1] {
        if piece.is_empty() {
            continue;
        }
        let pos = if caseless {
            ascii_casestr(rest, piece)
        } else {
            memmem::find(rest, piece)
        };
        match pos {
            Some(i) => rest = &rest[i + piece.len()..],
            None => return false,
        }
    }
    true
}

fn new_parent(term: &[u8], flags: u32) -> Leaf {
    if su::fsearch_string_is_ascii_icase(Some(term)) || flags & MATCH_CASE != 0 {
        let op = if flags & MATCH_CASE != 0 {
            AsciiOp::StrCmp
        } else {
            AsciiOp::StrCaseCmp
        };
        leaf(
            flags,
            Matcher::Ascii {
                needle: term.to_vec(),
                finder: memmem::Finder::new(term).into_owned(),
                op,
                hay: Hay::ParentPath,
            },
        )
    } else {
        match utf::fold_needle(term) {
            Some(needle) => leaf(
                flags,
                Matcher::Utf {
                    needle_ascii: needle.is_ascii(),
                    needle,
                    exact: true,
                    hay: Hay::ParentPath,
                },
            ),
            None => match_nothing(),
        }
    }
}

fn ascii_casecmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let n = a.len().min(b.len());
    for i in 0..n {
        let (x, y) = (a[i].to_ascii_lowercase(), b[i].to_ascii_lowercase());
        if x != y {
            return x.cmp(&y);
        }
    }
    a.len().cmp(&b.len())
}

fn new_extension(term: Option<&[u8]>, flags: u32) -> Leaf {
    let flags = flags | FILES_ONLY;
    let case = flags & MATCH_CASE != 0;
    let terms = match term {
        None => vec![Vec::new()],
        Some(t) => {
            let mut v: Vec<Vec<u8>> = t.split(|&b| b == b';').map(|s| s.to_vec()).collect();
            if case {
                v.sort();
            } else {
                v.sort_by(|a, b| ascii_casecmp(a, b));
            }
            v
        }
    };
    let mut set: Vec<Vec<u8>> = terms
        .iter()
        .map(|t| {
            if case {
                t.clone()
            } else {
                t.to_ascii_lowercase()
            }
        })
        .collect();
    set.sort();
    set.dedup();
    leaf(flags, Matcher::Ext { terms, set, case })
}

fn new_string_comparison(term: &[u8], flags: u32, ascii_haystack: bool) -> Leaf {
    let hay = if flags & SEARCH_IN_PATH != 0 {
        Hay::Path
    } else {
        Hay::Name
    };
    if ascii_haystack || su::fsearch_string_is_ascii_icase(Some(term)) || flags & MATCH_CASE != 0 {
        let case = flags & MATCH_CASE != 0;
        let op = match (flags & EXACT_MATCH != 0, case) {
            (true, true) => AsciiOp::StrCmp,
            (true, false) => AsciiOp::StrCaseCmp,
            (false, true) => AsciiOp::StrStr,
            (false, false) => AsciiOp::StrCaseStr,
        };
        leaf(
            flags,
            Matcher::Ascii {
                needle: term.to_vec(),
                finder: memmem::Finder::new(term).into_owned(),
                op,
                hay,
            },
        )
    } else {
        match utf::fold_needle(term) {
            Some(needle) => leaf(
                flags,
                Matcher::Utf {
                    needle_ascii: needle.is_ascii(),
                    needle,
                    exact: flags & EXACT_MATCH != 0,
                    hay,
                },
            ),
            None => match_nothing(),
        }
    }
}

fn new_contenttype(term: &[u8], flags: u32) -> Leaf {
    let mut res = if flags & REGEX != 0 {
        new_regex(term, flags)
    } else if su::fsearch_string_has_wildcards(Some(term)) {
        new_wildcard(term, flags)
    } else {
        new_string_comparison(term, flags, true)
    };
    match &mut res.matcher {
        Matcher::Regex { hay, glob, .. } => {
            *hay = Hay::ContentType;
            *glob = None;
        }
        Matcher::Ascii { hay, .. } | Matcher::Utf { hay, .. } => *hay = Hay::ContentType,
        _ => {}
    }
    res.wants_single_threaded_search = true;
    res
}

/// `fsearch_query_node_new`.
fn new_node(term: &[u8], mut flags: u32) -> Leaf {
    let has_sep = term.contains(&b'/');
    let trig_case = flags & MATCH_CASE == 0
        && flags & AUTO_MATCH_CASE != 0
        && su::fsearch_string_utf8_has_upper(Some(term));
    let trig_path = flags & SEARCH_IN_PATH == 0 && flags & AUTO_SEARCH_IN_PATH != 0 && has_sep;
    if trig_path {
        flags |= SEARCH_IN_PATH;
    }
    if trig_case {
        flags |= MATCH_CASE;
    }
    let mut res = if flags & REGEX != 0 {
        new_regex(term, flags)
    } else if su::fsearch_string_has_wildcards(Some(term)) {
        new_wildcard(term, flags)
    } else {
        new_string_comparison(term, flags, false)
    };
    res.triggers_auto_match_case = trig_case;
    res.triggers_auto_match_path = trig_path;
    res
}

// ---------------------------------------------------------------------------------------------
// Parser

const MODIFIERS: &[(&str, u32, bool)] = &[
    ("case", MATCH_CASE, true),
    ("nocase", MATCH_CASE, false),
    ("exact", EXACT_MATCH, true),
    ("file", FILES_ONLY, true),
    ("nofileonly", FILES_ONLY, false),
    ("files", FILES_ONLY, true),
    ("nofilesonly", FILES_ONLY, false),
    ("folder", FOLDERS_ONLY, true),
    ("nofolderonly", FOLDERS_ONLY, false),
    ("folders", FOLDERS_ONLY, true),
    ("nofoldersonly", FOLDERS_ONLY, false),
    ("path", SEARCH_IN_PATH, true),
    ("nopath", SEARCH_IN_PATH, false),
    ("regex", REGEX, true),
    ("noregex", REGEX, false),
];

#[derive(Clone, Copy)]
enum Function {
    ChildCount,
    ChildFileCount,
    ChildFolderCount,
    ContentType,
    Depth,
    DateModified,
    Empty,
    Ext,
    Parent,
    Size,
}

const FUNCTIONS: &[(&str, Function)] = &[
    ("childcount", Function::ChildCount),
    ("childfilecount", Function::ChildFileCount),
    ("childfoldercount", Function::ChildFolderCount),
    ("contenttype", Function::ContentType),
    ("depth", Function::Depth),
    ("dm", Function::DateModified),
    ("datemodified", Function::DateModified),
    ("empty", Function::Empty),
    ("ext", Function::Ext),
    ("parent", Function::Parent),
    ("parents", Function::Depth),
    ("size", Function::Size),
];

#[derive(Clone, Copy)]
enum ValueParser {
    Integer,
    Size,
    Date,
}

fn parse_integer(s: &[u8]) -> Option<(i64, i64)> {
    let (n, end) = size::strtoll(s);
    if end == 0 || end != s.len() {
        return None;
    }
    Some((n, n))
}

impl ValueParser {
    fn parse(self, s: &[u8]) -> Option<(i64, i64)> {
        match self {
            ValueParser::Integer => parse_integer(s),
            ValueParser::Size => size::parse(s),
            ValueParser::Date => time::parse_interval(s),
        }
    }
}

fn is_operand(t: Token) -> bool {
    matches!(t, Token::Word | Token::Field | Token::FieldEmpty)
}

fn is_binary(t: Token) -> bool {
    matches!(t, Token::And | Token::Or)
}

fn precedence(t: Token) -> u32 {
    match t {
        Token::Not => 3,
        Token::And => 2,
        Token::Or => 1,
        _ => 0,
    }
}

fn op_node(t: Token) -> Option<PNode> {
    match t {
        Token::And => Some(PNode::Op(OpKind::And)),
        Token::Or => Some(PNode::Op(OpKind::Or)),
        Token::Not => Some(PNode::Op(OpKind::Not)),
        _ => None,
    }
}

struct Parser<'a> {
    lexer: Lexer,
    op_stack: Vec<Token>,
    last_token: Token,
    macros: Vec<&'a Filter>,
    macro_stack: Vec<usize>,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn parse_operator(&mut self, token: Token) -> Vec<PNode> {
        self.last_token = token;
        let mut res = Vec::new();
        while let Some(&top) = self.op_stack.last() {
            if precedence(token) > precedence(top) {
                break;
            }
            self.op_stack.pop();
            res.extend(op_node(top));
        }
        self.op_stack.push(token);
        res
    }

    fn implicit_and(&mut self, last: Token, next: Token) -> Vec<PNode> {
        if (is_operand(last) || last == Token::BracketClose)
            && (is_operand(next) || next == Token::BracketOpen || next == Token::Not)
        {
            return self.parse_operator(Token::And);
        }
        Vec::new()
    }

    fn followed_by_operand(&self, op: Token) -> bool {
        let next = self.lexer.peek();
        if is_binary(op) && next == Token::Not {
            return true;
        }
        is_operand(next) || next == Token::BracketOpen
    }

    fn consume_consecutive_not(&mut self) -> bool {
        let mut uneven = true;
        while self.lexer.peek() == Token::Not {
            self.lexer.next_token();
            uneven = !uneven;
        }
        uneven
    }

    fn discard_consecutive_binary(&mut self) {
        while is_binary(self.lexer.peek()) {
            self.lexer.next_token();
        }
    }

    fn parse_close_bracket(&mut self) -> Vec<PNode> {
        let mut res = Vec::new();
        loop {
            match self.op_stack.last() {
                Some(Token::BracketOpen) => {
                    self.op_stack.pop();
                    break;
                }
                // The original asserts here; the parser never reaches it with matched brackets.
                None => break,
                Some(&t) => {
                    self.op_stack.pop();
                    res.extend(op_node(t));
                }
            }
        }
        self.last_token = Token::BracketClose;
        res
    }

    fn parse_open_bracket(&mut self) -> Vec<PNode> {
        self.last_token = Token::BracketOpen;
        self.op_stack.push(Token::BracketOpen);
        Vec::new()
    }

    fn parse_expression(&mut self, in_open_bracket: bool, flags: u32) -> Vec<PNode> {
        if self.depth >= MAX_PARSE_DEPTH {
            return vec![PNode::Leaf(Box::new(match_nothing()))];
        }
        self.depth += 1;
        let res = self.parse_expression_inner(in_open_bracket, flags);
        self.depth -= 1;
        res
    }

    fn parse_expression_inner(&mut self, in_open_bracket: bool, flags: u32) -> Vec<PNode> {
        let mut res: Vec<PNode> = Vec::new();
        let mut num_open: u32 = in_open_bracket as u32;
        let mut num_close: u32 = 0;
        loop {
            let (token, value) = self.lexer.next_token();
            let last = self.last_token;
            let implicit = if is_operand(token) || token == Token::BracketOpen {
                self.implicit_and(last, token)
            } else {
                Vec::new()
            };
            let mut to_append: Vec<PNode> = Vec::new();
            match token {
                Token::Eos => break,
                Token::Not => {
                    if self.consume_consecutive_not() && self.followed_by_operand(token) {
                        to_append = self.implicit_and(last, token);
                        to_append.extend(self.parse_operator(token));
                    }
                }
                Token::And | Token::Or => {
                    if self.followed_by_operand(token) {
                        to_append = self.parse_operator(token);
                    }
                }
                Token::BracketOpen => {
                    num_open += 1;
                    to_append = self.parse_open_bracket();
                    self.discard_consecutive_binary();
                }
                Token::BracketClose => {
                    if num_open > num_close {
                        num_close += 1;
                        to_append = self.parse_close_bracket();
                        if in_open_bracket && num_close == num_open {
                            res.extend(to_append);
                            return res;
                        }
                    } else {
                        return vec![PNode::Leaf(Box::new(match_nothing()))];
                    }
                }
                Token::Word => to_append = vec![PNode::Leaf(Box::new(new_node(&value, flags)))],
                Token::Field => to_append = self.parse_field(&value, false, flags),
                Token::FieldEmpty => to_append = self.parse_field(&value, true, flags),
                _ => {}
            }
            res.extend(implicit);
            if !to_append.is_empty() {
                self.last_token = token;
                res.extend(to_append);
            }
        }
        while let Some(t) = self.op_stack.pop() {
            res.extend(op_node(t));
        }
        res
    }

    fn parse_filter_macros(&mut self, name: &[u8], mut flags: u32) -> Vec<PNode> {
        let mut res = Vec::new();
        for (i, filter) in self.macros.clone().into_iter().enumerate() {
            if name != filter.macro_name.as_bytes() {
                continue;
            }
            if self.macro_stack.contains(&i) {
                break;
            }
            if su::fsearch_string_is_empty(Some(filter.query.as_bytes())) {
                break;
            }
            flags |= filter.flags & (SEARCH_IN_PATH | MATCH_CASE | REGEX);
            self.macro_stack.push(i);
            let main_lexer =
                std::mem::replace(&mut self.lexer, Lexer::new(filter.query.as_bytes()));
            let main_ops = std::mem::take(&mut self.op_stack);
            res = self.parse_expression(false, flags);
            self.lexer = main_lexer;
            self.op_stack = main_ops;
            self.macro_stack.pop();
            break;
        }
        res
    }

    fn parse_field(&mut self, name: &[u8], is_empty: bool, flags: u32) -> Vec<PNode> {
        let res = self.parse_filter_macros(name, flags);
        if res.is_empty() {
            for &(m, flag, add) in MODIFIERS {
                if m.as_bytes() == name {
                    let flags = if add { flags | flag } else { flags & !flag };
                    return self.parse_modifier(is_empty, flags);
                }
            }
            for &(f, func) in FUNCTIONS {
                if f.as_bytes() == name {
                    return self.parse_function(func, is_empty, flags);
                }
            }
        }
        if res.is_empty() {
            vec![PNode::Leaf(Box::new(match_nothing()))]
        } else {
            res
        }
    }

    fn parse_modifier(&mut self, is_empty: bool, flags: u32) -> Vec<PNode> {
        if is_empty {
            return vec![PNode::Leaf(Box::new(match_everything(flags)))];
        }
        let (token, value) = self.lexer.next_token();
        match token {
            Token::Word => vec![PNode::Leaf(Box::new(new_node(&value, flags)))],
            Token::BracketOpen => {
                let mut res = self.parse_open_bracket();
                res.extend(self.parse_expression(true, flags));
                res
            }
            Token::Field => self.parse_field(&value, false, flags),
            Token::FieldEmpty => self.parse_field(&value, true, flags),
            _ => vec![PNode::Leaf(Box::new(match_nothing()))],
        }
    }

    fn expect_word(&mut self) -> Option<Vec<u8>> {
        let (t, v) = self.lexer.next_token();
        (t == Token::Word).then_some(v)
    }

    fn parse_function(&mut self, func: Function, is_empty: bool, flags: u32) -> Vec<PNode> {
        let one = |l: Leaf| vec![PNode::Leaf(Box::new(l))];
        match func {
            Function::Size => self.parse_numeric(is_empty, flags, NumProp::Size, ValueParser::Size),
            Function::Depth => {
                self.parse_numeric(is_empty, flags, NumProp::Depth, ValueParser::Integer)
            }
            Function::ChildCount => {
                self.parse_numeric(is_empty, flags, NumProp::ChildCount, ValueParser::Integer)
            }
            Function::ChildFileCount => self.parse_numeric(
                is_empty,
                flags,
                NumProp::ChildFileCount,
                ValueParser::Integer,
            ),
            Function::ChildFolderCount => self.parse_numeric(
                is_empty,
                flags,
                NumProp::ChildFolderCount,
                ValueParser::Integer,
            ),
            Function::DateModified => {
                self.parse_numeric(is_empty, flags, NumProp::DateModified, ValueParser::Date)
            }
            Function::Empty => one(new_numeric(NumProp::ChildCount, flags, 0, 0, Cmp::Equal)),
            Function::Ext => {
                if is_empty {
                    return one(new_extension(None, flags));
                }
                match self.expect_word() {
                    Some(v) => one(new_extension(Some(&v), flags)),
                    None => one(match_nothing()),
                }
            }
            Function::ContentType => {
                if is_empty {
                    return one(match_everything(flags));
                }
                match self.expect_word() {
                    Some(v) => one(new_contenttype(&v, flags)),
                    None => one(match_nothing()),
                }
            }
            Function::Parent => {
                let pf = flags | EXACT_MATCH;
                if is_empty {
                    return one(new_parent(b"", pf));
                }
                match self.expect_word() {
                    Some(v) => one(new_parent(&v, pf)),
                    None => one(match_nothing()),
                }
            }
        }
    }

    fn parse_numeric(
        &mut self,
        is_empty: bool,
        flags: u32,
        prop: NumProp,
        vp: ValueParser,
    ) -> Vec<PNode> {
        let one = |l: Leaf| vec![PNode::Leaf(Box::new(l))];
        if is_empty {
            return one(match_everything(flags));
        }
        let (token, value) = self.lexer.next_token();
        let cmp = match token {
            Token::Equal => Cmp::Equal,
            Token::Smaller => Cmp::Smaller,
            Token::SmallerEq => Cmp::SmallerEq,
            Token::Greater => Cmp::Greater,
            Token::GreaterEq => Cmp::GreaterEq,
            Token::Word => return one(numeric_with_optional_range(prop, vp, &value, flags)),
            _ => return one(match_nothing()),
        };
        if let Some(v) = self.expect_word()
            && let Some((a, b)) = vp.parse(&v)
        {
            return one(new_numeric(prop, flags, a, b, cmp));
        }
        one(match_nothing())
    }
}

/// `g_strsplit(s, "..", 2)`.
fn split_range(s: &[u8]) -> Vec<&[u8]> {
    if s.is_empty() {
        return Vec::new();
    }
    match memmem::find(s, b"..") {
        Some(i) => vec![&s[..i], &s[i + 2..]],
        None => vec![s],
    }
}

fn numeric_with_optional_range(prop: NumProp, vp: ValueParser, s: &[u8], flags: u32) -> Leaf {
    let parts = split_range(s);
    if parts.is_empty() {
        return match_nothing();
    }
    let mut start: i64 = 0;
    let mut end: i64 = 0;
    let mut cmp;
    if su::fsearch_string_is_empty(Some(parts[0])) {
        cmp = Cmp::Range;
    } else {
        match vp.parse(parts[0]) {
            Some((a, b)) => {
                start = a;
                end = b;
                cmp = Cmp::Range;
            }
            None => return match_nothing(),
        }
    }
    if parts.len() > 1 {
        if su::fsearch_string_is_empty(Some(parts[1])) {
            end = i32::MAX as i64;
            cmp = Cmp::GreaterEq;
        } else {
            match vp.parse(parts[1]) {
                Some((_, b)) => {
                    end = b;
                    cmp = Cmp::Range;
                }
                None => return match_nothing(),
            }
        }
    }
    if start == end {
        cmp = Cmp::Equal;
    }
    new_numeric(prop, flags, start, end, cmp)
}

/// `left op right`, merged into an existing list of the same operator.
fn join(op: OpKind, mut left: Tree, mut right: Tree) -> Tree {
    let and = op == OpKind::And;
    let same = |t: &Tree| matches!((t, and), (Tree::And(_), true) | (Tree::Or(_), false));
    // reuse the left list (chains grow on the left), so a long chain is built in linear time
    let mut items = if same(&left) {
        match &mut left {
            Tree::And(v) | Tree::Or(v) => std::mem::take(v),
            _ => Vec::new(),
        }
    } else {
        vec![std::mem::replace(&mut left, Tree::And(Vec::new()))]
    };
    if same(&right) {
        if let Tree::And(v) | Tree::Or(v) = &mut right {
            items.append(v);
        }
    } else {
        items.push(std::mem::replace(&mut right, Tree::And(Vec::new())));
    }
    if and {
        Tree::And(items)
    } else {
        Tree::Or(items)
    }
}

fn tree_depth(t: &Tree) -> usize {
    let mut max = 0;
    let mut stack = vec![(t, 1usize)];
    while let Some((n, d)) = stack.pop() {
        max = max.max(d);
        match n {
            Tree::And(v) | Tree::Or(v) => stack.extend(v.iter().map(|c| (c, d + 1))),
            Tree::Not(c) => stack.push((c, d + 1)),
            Tree::Leaf(_) => {}
        }
    }
    max
}

fn build_tree(postfix: Vec<PNode>, flags: u32) -> Tree {
    if postfix.is_empty() {
        return Tree::Leaf(Box::new(match_everything(flags)));
    }
    let everything = || Tree::Leaf(Box::new(match_everything(flags)));
    let mut stack: Vec<Tree> = Vec::new();
    for n in postfix {
        match n {
            PNode::Op(op) => {
                let right = stack.pop().unwrap_or_else(everything);
                if op == OpKind::Not {
                    stack.push(Tree::Not(Box::new(right)));
                } else {
                    let left = stack.pop().unwrap_or_else(everything);
                    stack.push(join(op, left, right));
                }
            }
            PNode::Leaf(l) => stack.push(Tree::Leaf(l)),
        }
    }
    let t = stack.pop().unwrap_or_else(everything);
    if tree_depth(&t) > MAX_TREE_DEPTH {
        return Tree::Leaf(Box::new(match_nothing()));
    }
    t
}

/// `g_strstrip`.
fn strip(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|&c| !g_ascii_isspace(c))
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&c| !g_ascii_isspace(c))
        .map_or(start, |e| e + 1);
    &s[start..end.max(start)]
}

/// `fsearch_query_node_tree_new`.
pub fn tree_new(term: &[u8], filters: &[Filter], flags: u32) -> Tree {
    let q = strip(term);
    if flags & REGEX != 0 {
        return Tree::Leaf(Box::new(new_node(q, flags)));
    }
    let macros: Vec<&Filter> = filters
        .iter()
        .filter(|f| {
            !f.macro_name.is_empty() && !su::fsearch_string_is_empty(Some(f.macro_name.as_bytes()))
        })
        .collect();
    let mut p = Parser {
        lexer: Lexer::new(q),
        op_stack: Vec::new(),
        last_token: Token::None,
        macros,
        macro_stack: Vec::new(),
        depth: 0,
    };
    let postfix = p.parse_expression(false, flags);
    build_tree(postfix, flags)
}

/// `new` matches only entries `old` matches (both single leaves).
fn leaf_refines(new: &Leaf, old: &Leaf) -> bool {
    // the type restriction may only get stricter
    for f in [FILES_ONLY, FOLDERS_ONLY] {
        if old.flags & f != 0 && new.flags & f == 0 {
            return false;
        }
    }
    match (&new.matcher, &old.matcher) {
        (_, Matcher::True) => true,
        (
            Matcher::Ascii {
                needle: n,
                op: nop,
                hay: nh,
                ..
            },
            Matcher::Ascii {
                needle: o,
                op: oop,
                hay: oh,
                ..
            },
        ) if nh == oh => match (nop, oop) {
            // a name containing the longer needle contains the shorter one
            (AsciiOp::StrStr, AsciiOp::StrStr) => memmem::find(n, o).is_some(),
            (AsciiOp::StrStr | AsciiOp::StrCaseStr, AsciiOp::StrCaseStr) => {
                ascii_casestr(n, o).is_some()
            }
            _ => false,
        },
        (
            Matcher::Utf {
                needle: n,
                exact: false,
                hay: nh,
                ..
            },
            Matcher::Utf {
                needle: o,
                exact: false,
                hay: oh,
                ..
            },
        ) if nh == oh => n.contains(o.as_str()),
        _ => false,
    }
}

impl Tree {
    /// The leaves of a top-level AND chain. With `strict`, any OR/NOT part makes it `None`;
    /// otherwise such parts are skipped (they can only narrow the result further).
    fn conjunct_leaves(&self, strict: bool) -> Option<Vec<&Leaf>> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(t) = stack.pop() {
            match t {
                Tree::And(v) => stack.extend(v.iter()),
                Tree::Leaf(l) => out.push(&**l),
                _ if strict => return None,
                _ => {}
            }
        }
        Some(out)
    }

    fn any(&self, f: &dyn Fn(&Leaf) -> bool) -> bool {
        match self {
            Tree::And(v) | Tree::Or(v) => v.iter().any(|c| c.any(f)),
            Tree::Not(a) => a.any(f),
            Tree::Leaf(l) => f(l),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Query + matching

pub struct Query {
    pub search_term: Vec<u8>,
    pub tree: Tree,
    pub filter: Option<Filter>,
    pub filter_tree: Option<Tree>,
    pub flags: u32,
    pub triggers_auto_match_case: bool,
    pub triggers_auto_match_path: bool,
    pub wants_single_threaded_search: bool,
    /// Some matcher reads full or parent paths (then the store's folder-path cache is used).
    pub uses_paths: bool,
}

impl Query {
    pub fn new(term: &str, filter: Option<&Filter>, filters: &[Filter], flags: u32) -> Query {
        Query::new_bytes(term.as_bytes(), filter, filters, flags)
    }

    pub fn new_bytes(
        term: &[u8],
        filter: Option<&Filter>,
        filters: &[Filter],
        flags: u32,
    ) -> Query {
        let tree = tree_new(term, filters, flags);
        let filter_tree = filter.map(|f| tree_new(f.query.as_bytes(), filters, f.flags));
        let path_leaf = |l: &Leaf| {
            matches!(
                &l.matcher,
                Matcher::Regex {
                    hay: Hay::Path | Hay::ParentPath,
                    ..
                } | Matcher::Ascii {
                    hay: Hay::Path | Hay::ParentPath,
                    ..
                } | Matcher::Utf {
                    hay: Hay::Path | Hay::ParentPath,
                    ..
                }
            )
        };
        let uses_paths =
            tree.any(&path_leaf) || filter_tree.as_ref().is_some_and(|t| t.any(&path_leaf));
        Query {
            uses_paths,
            search_term: term.to_vec(),
            triggers_auto_match_case: tree.any(&|l| l.triggers_auto_match_case),
            triggers_auto_match_path: tree.any(&|l| l.triggers_auto_match_path),
            wants_single_threaded_search: tree.any(&|l| l.wants_single_threaded_search),
            tree,
            filter: filter.cloned(),
            filter_tree,
            flags,
        }
    }

    /// True when every entry this query matches is also matched by `prev` (so searching the
    /// previous results gives the same answer as searching everything). Conservative: only
    /// AND-combinations of plain substring terms are compared; anything else answers false.
    pub fn refines(&self, prev: &Query) -> bool {
        if self.filter != prev.filter {
            return false;
        }
        if prev.matches_everything() {
            return true;
        }
        let Some(old) = prev.tree.conjunct_leaves(true) else {
            return false;
        };
        let new = self.tree.conjunct_leaves(false).unwrap_or_default();
        old.iter().all(|o| new.iter().any(|n| leaf_refines(n, o)))
    }

    /// `fsearch_query_matches_everything`.
    pub fn matches_everything(&self) -> bool {
        su::fsearch_string_is_empty(Some(&self.search_term))
            && self
                .filter
                .as_ref()
                .is_none_or(|f| su::fsearch_string_is_empty(Some(f.query.as_bytes())))
    }

    /// `fsearch_query_match`.
    pub fn matches(&self, md: &mut MatchData) -> bool {
        if let (Some(f), Some(ft)) = (&self.filter, &self.filter_tree)
            && !su::fsearch_string_is_empty(Some(f.query.as_bytes()))
            && !eval(ft, md)
        {
            return false;
        }
        eval(&self.tree, md)
    }

    /// Bold ranges in the name (and whether the path matched) for the result list.
    pub fn highlight(&self, md: &mut MatchData) -> Highlights {
        let mut h = Highlights::default();
        highlight_tree(&self.tree, md, &mut h);
        h
    }
}

fn eval(t: &Tree, md: &mut MatchData) -> bool {
    match t {
        Tree::And(v) => v.iter().all(|c| eval(c, md)),
        Tree::Or(v) => v.iter().any(|c| eval(c, md)),
        Tree::Not(a) => !eval(a, md),
        Tree::Leaf(l) => {
            let is_folder = md.id.is_folder();
            if l.flags & FOLDERS_ONLY != 0 && !is_folder {
                return false;
            }
            if l.flags & FILES_ONLY != 0 && is_folder {
                return false;
            }
            leaf_matches(l, md)
        }
    }
}

#[inline]
fn cmp_num(num: i64, cmp: Cmp, start: i64, end: i64) -> bool {
    match cmp {
        Cmp::Equal => num == start,
        Cmp::Greater => num > start,
        Cmp::Smaller => num < start,
        Cmp::GreaterEq => num >= start,
        Cmp::SmallerEq => num <= start,
        Cmp::Range => start <= num && num < end,
    }
}

/// glibc `strcasestr` in a UTF-8 locale: ASCII-only case folding, byte-wise.
pub fn ascii_casestr(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > hay.len() {
        return None;
    }
    let first = needle[0].to_ascii_lowercase();
    let first_up = needle[0].to_ascii_uppercase();
    let last_start = hay.len() - needle.len();
    let mut i = 0;
    while i <= last_start {
        let rest = &hay[i..=last_start];
        let off = if first == first_up {
            memchr::memchr(first, rest)
        } else {
            memchr::memchr2(first, first_up, rest)
        };
        let off = off?;
        let pos = i + off;
        if hay[pos..pos + needle.len()].eq_ignore_ascii_case(needle) {
            return Some(pos);
        }
        i = pos + 1;
    }
    None
}

fn leaf_matches(l: &Leaf, md: &mut MatchData) -> bool {
    match &l.matcher {
        Matcher::False => false,
        Matcher::True => true,
        Matcher::Regex { re, hay, glob } => {
            let h = md.hay(*hay);
            if let Some((p, caseless)) = glob
                && h.is_ascii()
                && !h.contains(&b'\n')
            {
                return glob_ascii(p, h, *caseless);
            }
            re.is_match(h)
        }
        Matcher::Ascii {
            needle,
            finder,
            op,
            hay,
        } => {
            let h = md.hay(*hay);
            match op {
                AsciiOp::StrStr => finder.find(h).is_some(),
                AsciiOp::StrCaseStr => ascii_casestr(h, needle).is_some(),
                AsciiOp::StrCmp => h == needle.as_slice(),
                AsciiOp::StrCaseCmp => h.eq_ignore_ascii_case(needle),
            }
        }
        Matcher::Utf {
            needle,
            needle_ascii,
            exact,
            hay,
        } => {
            // Fast path: outside Turkic locales an ASCII name folds to its ASCII lowercase and NFD
            // leaves it unchanged, so ICU is only needed for non-ASCII names.
            if !utf::turkic() && md.hay(*hay).is_ascii() {
                if !needle_ascii {
                    return false;
                }
                let lower = md.lower_ascii(*hay);
                if lower.len() > utf::HAYSTACK_CAPACITY {
                    return false;
                }
                return if *exact {
                    lower == needle.as_bytes()
                } else {
                    memmem::find(lower, needle.as_bytes()).is_some()
                };
            }
            match md.folded(*hay) {
                Some(h) => {
                    if *exact {
                        h == needle.as_str()
                    } else {
                        memmem::find(h.as_bytes(), needle.as_bytes()).is_some()
                    }
                }
                None => false,
            }
        }
        Matcher::Ext { terms, set, case } => {
            let Some(ext) = md.db.extension(md.id) else {
                return false;
            };
            if terms.len() > 8 {
                // long lists (the filter macros): binary search over the sorted (lowercased) terms
                return if *case {
                    set.binary_search_by(|t| t.as_slice().cmp(ext)).is_ok()
                } else {
                    set.binary_search_by(|t| {
                        t.iter()
                            .copied()
                            .cmp(ext.iter().map(u8::to_ascii_lowercase))
                    })
                    .is_ok()
                };
            }
            // the original's loop: terms are sorted, so stop at the first larger one
            for t in terms {
                let r = if *case {
                    ext.cmp(t.as_slice())
                } else {
                    ascii_casecmp(ext, t)
                };
                match r {
                    std::cmp::Ordering::Equal => return true,
                    std::cmp::Ordering::Less => return false,
                    std::cmp::Ordering::Greater => {}
                }
            }
            false
        }
        Matcher::Num {
            prop,
            cmp,
            start,
            end,
        } => {
            let e = md.db.entry(md.id);
            let is_folder = md.id.is_folder();
            let v = match prop {
                NumProp::DateModified => e.mtime,
                NumProp::Size => e.size,
                NumProp::Depth => md.db.depth(md.id) as i64,
                NumProp::ChildCount if is_folder => (e.num_files + e.num_folders) as i64,
                NumProp::ChildFileCount if is_folder => e.num_files as i64,
                NumProp::ChildFolderCount if is_folder => e.num_folders as i64,
                _ => return false,
            };
            cmp_num(v, *cmp, *start, *end)
        }
    }
}

/// Per-thread scratch state for matching one entry at a time (`FsearchQueryMatchData`).
pub struct MatchData<'a> {
    pub db: &'a Db,
    pub id: Id,
    path: Vec<u8>,
    path_ready: bool,
    parent: Vec<u8>,
    parent_ready: bool,
    ctype: Vec<u8>,
    ctype_ready: bool,
    /// Folded+normalized haystacks: buffers reused across entries; state 0 = not computed,
    /// 1 = ready, 2 = folding failed (no match).
    folded: [String; 3],
    folded_state: [u8; 3],
    fold_tmp: String,
    lower: Vec<u8>,
    /// Cached folder paths (`Store::folder_paths`), when the query looks at paths.
    paths: Option<&'a crate::db::FolderPaths>,
}

impl<'a> MatchData<'a> {
    pub fn new(db: &'a Db) -> MatchData<'a> {
        MatchData {
            db,
            id: Id::File(0),
            path: Vec::with_capacity(256),
            path_ready: false,
            parent: Vec::with_capacity(256),
            parent_ready: false,
            ctype: Vec::new(),
            ctype_ready: false,
            folded: [String::new(), String::new(), String::new()],
            folded_state: [0; 3],
            fold_tmp: String::new(),
            lower: Vec::with_capacity(256),
            paths: None,
        }
    }

    /// Match data that builds paths from the store's folder-path cache.
    pub fn with_paths(db: &'a Db, paths: &'a crate::db::FolderPaths) -> MatchData<'a> {
        let mut md = MatchData::new(db);
        md.paths = Some(paths);
        md
    }

    #[inline]
    pub fn set(&mut self, id: Id) {
        self.id = id;
        self.path_ready = false;
        self.parent_ready = false;
        self.ctype_ready = false;
        self.folded_state = [0; 3];
    }

    /// ASCII lowercase copy of a haystack (scratch buffer).
    fn lower_ascii(&mut self, h: Hay) -> &[u8] {
        let mut buf = std::mem::take(&mut self.lower);
        buf.clear();
        buf.extend_from_slice(self.hay(h));
        buf.make_ascii_lowercase();
        self.lower = buf;
        &self.lower
    }

    pub fn path(&mut self) -> &[u8] {
        if !self.path_ready {
            self.path.clear();
            match self.paths {
                Some(fp) => fp.append_full_path(self.db, self.id, &mut self.path),
                None => self.db.append_full_path(self.id, &mut self.path),
            }
            self.path_ready = true;
        }
        &self.path
    }

    fn parent_path(&mut self) -> &[u8] {
        if !self.parent_ready {
            self.parent.clear();
            match self.paths {
                Some(fp) => fp.append_parent_path(self.db, self.id, &mut self.parent),
                None => self.db.append_parent_path(self.id, &mut self.parent),
            }
            self.parent_ready = true;
        }
        &self.parent
    }

    fn content_type(&mut self) -> &[u8] {
        if !self.ctype_ready {
            let p = self.db.full_path(self.id);
            self.ctype = crate::content::content_type_for_path(&p).into_bytes();
            self.ctype_ready = true;
        }
        &self.ctype
    }

    fn hay(&mut self, h: Hay) -> &[u8] {
        match h {
            Hay::Name => self.db.display_name(self.id),
            Hay::Path => self.path(),
            Hay::ParentPath => self.parent_path(),
            Hay::ContentType => self.content_type(),
        }
    }

    fn folded(&mut self, h: Hay) -> Option<&str> {
        let slot = match h {
            Hay::Name => 0,
            Hay::Path => 1,
            _ => 2,
        };
        if self.folded_state[slot] == 0 {
            let mut out = std::mem::take(&mut self.folded[slot]);
            let mut tmp = std::mem::take(&mut self.fold_tmp);
            let ok =
                utf::fold_normalize_into(self.hay(h), utf::HAYSTACK_CAPACITY, &mut tmp, &mut out);
            self.fold_tmp = tmp;
            self.folded[slot] = out;
            self.folded_state[slot] = if ok { 1 } else { 2 };
        }
        (self.folded_state[slot] == 1).then_some(self.folded[slot].as_str())
    }
}

/// Highlighted byte ranges for display.
#[derive(Default, Debug, Clone)]
pub struct Highlights {
    pub name: Vec<(usize, usize)>,
    pub path: Vec<(usize, usize)>,
}

fn add_path_highlight(md: &mut MatchData, start: usize, len: usize, h: &mut Highlights) {
    let name_len = md.db.display_name(md.id).len();
    let path_len = md.path().len();
    let parent_len = path_len.saturating_sub(name_len);
    if start > parent_len {
        let s = start - parent_len;
        h.name.push((s, s + len));
    } else if start + len > parent_len {
        h.path.push((start, path_len));
        h.name.push((0, start + len - parent_len));
    } else {
        h.path.push((start, start + len));
    }
}

fn highlight_tree(t: &Tree, md: &mut MatchData, h: &mut Highlights) -> bool {
    match t {
        Tree::And(v) => v.iter().all(|c| highlight_tree(c, md, h)),
        Tree::Or(v) => v.iter().any(|c| highlight_tree(c, md, h)),
        Tree::Not(a) => !highlight_tree(a, md, h),
        Tree::Leaf(l) => {
            let is_folder = md.id.is_folder();
            if (l.flags & FOLDERS_ONLY != 0 && !is_folder)
                || (l.flags & FILES_ONLY != 0 && is_folder)
            {
                return false;
            }
            let in_path = l.flags & SEARCH_IN_PATH != 0;
            match &l.matcher {
                Matcher::Ascii {
                    needle, op, hay, ..
                } if matches!(hay, Hay::Name | Hay::Path) => {
                    let hs = md.hay(*hay).to_vec();
                    let pos = match op {
                        AsciiOp::StrStr => memmem::find(&hs, needle),
                        AsciiOp::StrCaseStr => ascii_casestr(&hs, needle),
                        AsciiOp::StrCmp => (hs == *needle).then_some(0),
                        AsciiOp::StrCaseCmp => hs.eq_ignore_ascii_case(needle).then_some(0),
                    };
                    match pos {
                        Some(p) if matches!(op, AsciiOp::StrCmp | AsciiOp::StrCaseCmp) => {
                            h.name.push((0, md.db.display_name(md.id).len()));
                            let _ = p;
                            true
                        }
                        Some(p) => {
                            if in_path {
                                add_path_highlight(md, p, needle.len(), h);
                            } else {
                                h.name.push((p, p + needle.len()));
                            }
                            true
                        }
                        None => false,
                    }
                }
                Matcher::Regex { re, hay, .. } if matches!(hay, Hay::Name | Hay::Path) => {
                    let hs = md.hay(*hay).to_vec();
                    if std::str::from_utf8(&hs).is_err() {
                        return false;
                    }
                    match re.get().find(&hs) {
                        Ok(Some(m)) => {
                            if in_path {
                                add_path_highlight(md, m.start(), m.end() - m.start(), h);
                            } else {
                                h.name.push((m.start(), m.end()));
                            }
                            true
                        }
                        _ => false,
                    }
                }
                Matcher::Ext { .. } => {
                    if !leaf_matches(l, md) {
                        return false;
                    }
                    let name_len = md.db.display_name(md.id).len();
                    let ext_len = md.db.extension(md.id).map_or(0, |e| e.len());
                    h.name.push((name_len - ext_len, name_len));
                    true
                }
                _ => leaf_matches(l, md),
            }
        }
    }
}

#[cfg(test)]
mod glob_tests {
    use super::glob_ascii;

    #[test]
    fn globs_agree_with_backtracking() {
        let cases = [
            ("*.so", "libc.so", true),
            ("*.so", "libc.so.6", false),
            ("lib*", "libc.so", true),
            ("*a*b*", "xaxbx", true),
            ("*a*b*", "xbxax", false),
            ("a*b*a", "abba", true),
            ("a*a", "a", false),
            ("**", "", true),
            ("*.SO", "x.so", true),
        ];
        for (p, s, want) in cases {
            assert_eq!(
                glob_ascii(p.as_bytes(), s.as_bytes(), true),
                want,
                "{p} {s}"
            );
        }
        assert!(!glob_ascii(b"*.SO", b"x.so", false));
    }
}
