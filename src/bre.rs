// ved's BRE engine — compiled, with capturing groups and
// backreferences.
//
// Step 1 was a direct port of Pike's recursive matcher: three
// functions, no compile step, the pattern itself is the program.
// That original is preserved in git history (commit c53f626).
//
// Step 2 introduced a compile step (closer to Ken Thompson's
// original ed) and bracket expressions ([abc], [^abc], [a-z]).
//
// Step 3 wrapped the compiled form in a Regex struct, returning
// match positions via a Match struct.
//
// Step 4 added capturing groups: \(...\) in BRE syntax. The
// compiler emits GroupStart/GroupEnd markers, and a small Caps
// struct threads through the recursion by value so that failed
// branches automatically discard partial captures.
//
// Step 5 added backreferences: \1-\9 in the pattern replay
// whatever the corresponding group captured. The match functions
// now receive full_text so backreferences can look up captured
// substrings from earlier in the input.
//
// Known limitation: \(...\)* (star applied to a whole group) is
// not yet supported. Star can only apply to a single atom. This
// is uncommon in ed usage and can be added later if needed.

/// One matchable thing: a literal byte, any-byte wildcard, or
/// a bracket character class.
#[derive(Debug)]
enum Atom {
    Literal(u8),
    Dot,
    Class { negated: bool, chars: Vec<u8> },
}

/// One element of a compiled pattern.
#[derive(Debug)]
enum Element {
    One(Atom),
    Star(Atom),
    Caret,
    Dollar,
    GroupStart(usize),
    GroupEnd(usize),
    BackRef(usize),
}

/// Capture state for up to 9 groups, threaded through matching
/// by value. Small enough to copy on every recursive call (9
/// usizes + 9 usizes + 9 bools = ~153 bytes).
#[derive(Debug, Clone, Copy)]
struct Caps {
    start: [usize; 9],
    end: [usize; 9],
    valid: [bool; 9],
}

impl Caps {
    fn new() -> Self {
        Caps {
            start: [0; 9],
            end: [0; 9],
            valid: [false; 9],
        }
    }
}

/// A compiled BRE pattern, ready for matching.
#[derive(Debug)]
pub struct Regex {
    elements: Vec<Element>,
}

/// The result of a successful match: byte offsets into the text,
/// plus any captured groups.
#[derive(Debug)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    caps: Caps,
}

impl Match {
    /// Get the byte range of capturing group `n` (1-9).
    /// Returns None if the group wasn't part of the pattern or
    /// didn't participate in the match.
    pub fn group(&self, n: usize) -> Option<(usize, usize)> {
        if n >= 1 && n <= 9 && self.caps.valid[n - 1] {
            Some((self.caps.start[n - 1], self.caps.end[n - 1]))
        } else {
            None
        }
    }
}

// ── Compiler ─────────────────────────────────────────────────

impl Regex {
    /// Compile a BRE pattern from raw bytes.
    pub fn compile(pattern: &[u8]) -> Regex {
        let mut elements = Vec::new();
        let mut i = 0;
        let mut group_num: usize = 0;
        let mut group_stack: Vec<usize> = Vec::new();

        // ^ only anchors at the very start of the pattern.
        if i < pattern.len() && pattern[i] == b'^' {
            elements.push(Element::Caret);
            i += 1;
        }

        while i < pattern.len() {
            // $ only anchors at the very end of the pattern.
            if pattern[i] == b'$' && i + 1 == pattern.len() {
                elements.push(Element::Dollar);
                i += 1;
                continue;
            }

            // Backslash sequences: \( \) for groups, \1-\9 for
            // backreferences, \X for literal X.
            if pattern[i] == b'\\' && i + 1 < pattern.len() {
                match pattern[i + 1] {
                    b'(' => {
                        group_num += 1;
                        group_stack.push(group_num);
                        elements.push(Element::GroupStart(group_num));
                        i += 2;
                        continue;
                    }
                    b')' => {
                        if let Some(n) = group_stack.pop() {
                            elements.push(Element::GroupEnd(n));
                        }
                        i += 2;
                        continue;
                    }
                    c @ b'1'..=b'9' => {
                        let n = (c - b'0') as usize;
                        elements.push(Element::BackRef(n));
                        i += 2;
                        continue;
                    }
                    c => {
                        // \X is literal X (handles \., \*, \[, \\, etc.)
                        let atom = Atom::Literal(c);
                        i += 2;
                        if i < pattern.len() && pattern[i] == b'*' {
                            elements.push(Element::Star(atom));
                            i += 1;
                        } else {
                            elements.push(Element::One(atom));
                        }
                        continue;
                    }
                }
            }

            // Parse one atom.
            let (atom, next) = parse_atom(pattern, i);
            i = next;

            // If followed by *, wrap in Star; otherwise One.
            if i < pattern.len() && pattern[i] == b'*' {
                elements.push(Element::Star(atom));
                i += 1;
            } else {
                elements.push(Element::One(atom));
            }
        }

        Regex { elements }
    }

    // ── Matcher ──────────────────────────────────────────────

    /// Search for this pattern anywhere in `text`. Returns the
    /// position of the first match, or None.
    pub fn find(&self, text: &[u8]) -> Option<Match> {
        let elements = &self.elements;
        let anchored = !elements.is_empty()
            && matches!(elements[0], Element::Caret);
        let elems = if anchored { &elements[1..] } else { elements };

        let mut offset = 0;
        loop {
            let caps = Caps::new();
            if let Some((len, caps)) =
                self.match_here(elems, &text[offset..], offset, caps, text)
            {
                return Some(Match {
                    start: offset,
                    end: offset + len,
                    caps,
                });
            }
            if anchored || offset >= text.len() {
                return None;
            }
            offset += 1;
        }
    }

    /// Try to match `elements` at the beginning of `text`.
    /// `pos` is the absolute offset in the original text (needed
    /// for recording group boundaries). `full_text` is the
    /// complete original input (needed for backreference lookups).
    /// Returns the number of bytes consumed and the capture state
    /// on success.
    fn match_here(
        &self,
        elements: &[Element],
        text: &[u8],
        pos: usize,
        caps: Caps,
        full_text: &[u8],
    ) -> Option<(usize, Caps)> {
        if elements.is_empty() {
            return Some((0, caps));
        }
        match &elements[0] {
            Element::Dollar => {
                if text.is_empty() {
                    Some((0, caps))
                } else {
                    None
                }
            }
            Element::Caret => None,
            Element::GroupStart(n) => {
                let mut caps = caps;
                caps.start[n - 1] = pos;
                self.match_here(&elements[1..], text, pos, caps, full_text)
            }
            Element::GroupEnd(n) => {
                let mut caps = caps;
                caps.end[n - 1] = pos;
                caps.valid[n - 1] = true;
                self.match_here(&elements[1..], text, pos, caps, full_text)
            }
            Element::BackRef(n) => {
                if !caps.valid[n - 1] {
                    return None;
                }
                let captured = &full_text[caps.start[n - 1]..caps.end[n - 1]];
                if text.starts_with(captured) {
                    let clen = captured.len();
                    self.match_here(
                        &elements[1..],
                        &text[clen..],
                        pos + clen,
                        caps,
                        full_text,
                    )
                    .map(|(len, caps)| (len + clen, caps))
                } else {
                    None
                }
            }
            Element::Star(atom) => {
                self.match_star(atom, &elements[1..], text, pos, caps, full_text)
            }
            Element::One(atom) => {
                if !text.is_empty() && atom_matches(atom, text[0]) {
                    self.match_here(
                        &elements[1..],
                        &text[1..],
                        pos + 1,
                        caps,
                        full_text,
                    )
                    .map(|(len, caps)| (len + 1, caps))
                } else {
                    None
                }
            }
        }
    }

    /// Try zero, then one, then two, ... matches of `atom`,
    /// checking whether the remaining elements match after each
    /// count. Returns total bytes consumed and captures on success.
    fn match_star(
        &self,
        atom: &Atom,
        elements: &[Element],
        text: &[u8],
        pos: usize,
        caps: Caps,
        full_text: &[u8],
    ) -> Option<(usize, Caps)> {
        let mut consumed = 0;
        loop {
            if let Some((len, caps)) = self.match_here(
                elements,
                &text[consumed..],
                pos + consumed,
                caps,
                full_text,
            ) {
                return Some((consumed + len, caps));
            }
            if consumed >= text.len() {
                return None;
            }
            if !atom_matches(atom, text[consumed]) {
                return None;
            }
            consumed += 1;
        }
    }
}

/// Parse one atom starting at position `i` in the pattern.
/// Returns the atom and the index just past it.
fn parse_atom(pattern: &[u8], i: usize) -> (Atom, usize) {
    match pattern[i] {
        b'.' => (Atom::Dot, i + 1),
        b'[' => parse_bracket(pattern, i),
        c => (Atom::Literal(c), i + 1),
    }
}

/// Parse a bracket expression starting at the `[` at position
/// `start`. Returns a Class atom with the fully expanded
/// character list and the index just past the closing `]`.
///
/// Follows POSIX BRE bracket rules:
///   - `]` as the first character (after optional `^`) is literal
///   - `-` at the start or end is literal, not a range
///   - `a-z` in the middle is a range, expanded inline
///   - `[^...]` negates the class
fn parse_bracket(pattern: &[u8], start: usize) -> (Atom, usize) {
    let mut i = start + 1; // skip '['

    let negated = if i < pattern.len() && pattern[i] == b'^' {
        i += 1;
        true
    } else {
        false
    };

    let mut chars = Vec::new();

    // ] as the very first character (after optional ^) is literal.
    if i < pattern.len() && pattern[i] == b']' {
        chars.push(b']');
        i += 1;
    }

    while i < pattern.len() && pattern[i] != b']' {
        // Range: lo-hi, but only when - is followed by a char
        // that isn't the closing ].
        if i + 2 < pattern.len()
            && pattern[i + 1] == b'-'
            && pattern[i + 2] != b']'
        {
            let lo = pattern[i];
            let hi = pattern[i + 2];
            for c in lo..=hi {
                chars.push(c);
            }
            i += 3;
        } else {
            chars.push(pattern[i]);
            i += 1;
        }
    }

    // Skip the closing ].
    if i < pattern.len() {
        i += 1;
    }

    (Atom::Class { negated, chars }, i)
}

/// Does this atom match this byte?
fn atom_matches(atom: &Atom, byte: u8) -> bool {
    match atom {
        Atom::Literal(c) => byte == *c,
        Atom::Dot => true,
        Atom::Class { negated, chars } => {
            let found = chars.contains(&byte);
            if *negated { !found } else { found }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: compile and check for any match.
    fn has_match(pattern: &[u8], text: &[u8]) -> bool {
        Regex::compile(pattern).find(text).is_some()
    }

    // ── Step 1 tests ─────────────────────────────────────────

    // Literal matching

    #[test]
    fn literal_exact_match() {
        assert!(has_match(b"abc", b"abc"));
    }

    #[test]
    fn literal_match_in_middle() {
        assert!(has_match(b"abc", b"xabcy"));
    }

    #[test]
    fn literal_no_match() {
        assert!(!has_match(b"abc", b"xyz"));
    }

    #[test]
    fn literal_pattern_longer_than_text() {
        assert!(!has_match(b"abc", b"ab"));
    }

    #[test]
    fn empty_pattern_always_matches() {
        assert!(has_match(b"", b"hello"));
    }

    // Dot wildcard

    #[test]
    fn dot_matches_letter() {
        assert!(has_match(b"a.c", b"abc"));
    }

    #[test]
    fn dot_matches_underscore() {
        assert!(has_match(b"a.c", b"a_c"));
    }

    #[test]
    fn dot_requires_a_character() {
        assert!(!has_match(b"a.c", b"ac"));
    }

    // Star repetition

    #[test]
    fn star_zero_matches() {
        assert!(has_match(b"a*b", b"b"));
    }

    #[test]
    fn star_many_matches() {
        assert!(has_match(b"a*b", b"aaab"));
    }

    #[test]
    fn star_combined_with_slide() {
        assert!(has_match(b"a*b", b"xb"));
    }

    #[test]
    fn dot_star_matches_anything() {
        assert!(has_match(b".*", b"anything"));
    }

    // Anchors

    #[test]
    fn caret_anchors_at_start() {
        assert!(has_match(b"^abc", b"abc"));
    }

    #[test]
    fn caret_refuses_to_slide() {
        assert!(!has_match(b"^abc", b"xabc"));
    }

    #[test]
    fn dollar_anchors_at_end() {
        assert!(has_match(b"abc$", b"abc"));
    }

    #[test]
    fn dollar_refuses_tail() {
        assert!(!has_match(b"abc$", b"abcx"));
    }

    #[test]
    fn both_anchors() {
        assert!(has_match(b"^abc$", b"abc"));
    }

    #[test]
    fn empty_anchored_matches_empty() {
        assert!(has_match(b"^$", b""));
    }

    #[test]
    fn empty_anchored_rejects_nonempty() {
        assert!(!has_match(b"^$", b"x"));
    }

    // Tricky edge cases

    #[test]
    fn dollar_alone_finds_end() {
        assert!(has_match(b"$", b"anything"));
    }

    #[test]
    fn anchored_star_on_empty() {
        assert!(has_match(b"^a*$", b""));
    }

    #[test]
    fn anchored_star_many() {
        assert!(has_match(b"^a*$", b"aaa"));
    }

    #[test]
    fn anchored_star_rejects_wrong_char() {
        assert!(!has_match(b"^a*$", b"aab"));
    }

    // ── Step 2 tests: bracket expressions ────────────────────

    // Basic membership

    #[test]
    fn class_matches_member() {
        assert!(has_match(b"[abc]", b"b"));
    }

    #[test]
    fn class_rejects_nonmember() {
        assert!(!has_match(b"[abc]", b"d"));
    }

    #[test]
    fn class_in_context() {
        assert!(has_match(b"x[abc]y", b"xby"));
    }

    // Ranges

    #[test]
    fn range_matches_inside() {
        assert!(has_match(b"[a-z]", b"m"));
    }

    #[test]
    fn range_rejects_outside() {
        assert!(!has_match(b"[a-z]", b"M"));
    }

    #[test]
    fn multiple_ranges() {
        assert!(has_match(b"[a-zA-Z]", b"M"));
    }

    // Negation

    #[test]
    fn negated_class_matches_nonmember() {
        assert!(has_match(b"[^abc]", b"d"));
    }

    #[test]
    fn negated_class_rejects_member() {
        assert!(!has_match(b"^[^abc]$", b"a"));
    }

    #[test]
    fn negated_range_matches_outside() {
        assert!(has_match(b"[^0-9]", b"x"));
    }

    #[test]
    fn negated_range_rejects_inside() {
        assert!(!has_match(b"^[^0-9]$", b"5"));
    }

    // Edge cases: literal ] and -

    #[test]
    fn literal_close_bracket_first() {
        assert!(has_match(b"[]abc]", b"]"));
    }

    #[test]
    fn literal_dash_at_end() {
        assert!(has_match(b"[abc-]", b"-"));
    }

    #[test]
    fn literal_dash_at_start() {
        assert!(has_match(b"[-abc]", b"-"));
    }

    // Bracket + star

    #[test]
    fn class_star_zero() {
        assert!(has_match(b"^[a-z]*$", b""));
    }

    #[test]
    fn class_star_many() {
        assert!(has_match(b"^[a-z]*$", b"hello"));
    }

    #[test]
    fn class_star_rejects() {
        assert!(!has_match(b"^[a-z]*$", b"hello!"));
    }

    #[test]
    fn one_or_more_digits() {
        assert!(has_match(b"[0-9][0-9]*", b"abc123def"));
    }

    // ── Step 3 tests: match positions ────────────────────────

    #[test]
    fn position_at_start() {
        let m = Regex::compile(b"abc").find(b"abcdef").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn position_in_middle() {
        let m = Regex::compile(b"abc").find(b"xabcy").unwrap();
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 4);
    }

    #[test]
    fn position_dot_star() {
        let m = Regex::compile(b".*").find(b"hello").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 0);
    }

    #[test]
    fn position_anchored_dot_star() {
        let m = Regex::compile(b"^.*$").find(b"hello").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn position_digits() {
        let m = Regex::compile(b"[0-9][0-9]*").find(b"abc123def").unwrap();
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 4);
    }

    #[test]
    fn position_no_match() {
        assert!(Regex::compile(b"xyz").find(b"hello").is_none());
    }

    #[test]
    fn position_empty_match() {
        let m = Regex::compile(b"").find(b"hello").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 0);
    }

    #[test]
    fn position_dollar() {
        let m = Regex::compile(b"abc$").find(b"xabc").unwrap();
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 4);
    }

    // ── Step 4 tests: capturing groups ───────────────────────

    #[test]
    fn group_simple() {
        let m = Regex::compile(b"^\\(abc\\)$").find(b"abc").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
        assert_eq!(m.group(1), Some((0, 3)));
    }

    #[test]
    fn group_in_context() {
        let m = Regex::compile(b"^x\\(abc\\)y$").find(b"xabcy").unwrap();
        assert_eq!(m.group(1), Some((1, 4)));
    }

    #[test]
    fn group_two_groups() {
        let m = Regex::compile(b"^\\(ab\\)\\(cd\\)$").find(b"abcd").unwrap();
        assert_eq!(m.group(1), Some((0, 2)));
        assert_eq!(m.group(2), Some((2, 4)));
    }

    #[test]
    fn group_nested() {
        let m = Regex::compile(b"^\\(\\(ab\\)cd\\)$").find(b"abcd").unwrap();
        assert_eq!(m.group(1), Some((0, 4)));
        assert_eq!(m.group(2), Some((0, 2)));
    }

    #[test]
    fn group_with_dot() {
        let m = Regex::compile(b"^\\(.\\)$").find(b"x").unwrap();
        assert_eq!(m.group(1), Some((0, 1)));
    }

    #[test]
    fn group_with_class() {
        let m = Regex::compile(b"^\\([a-z][a-z]*\\)$").find(b"hello").unwrap();
        assert_eq!(m.group(1), Some((0, 5)));
    }

    #[test]
    fn group_no_match_returns_none() {
        assert!(Regex::compile(b"\\(abc\\)").find(b"xyz").is_none());
    }

    #[test]
    fn group_uncaptured_returns_none() {
        let m = Regex::compile(b"^\\(abc\\)$").find(b"abc").unwrap();
        assert_eq!(m.group(2), None);
    }

    #[test]
    fn group_zero_returns_none() {
        let m = Regex::compile(b"^\\(abc\\)$").find(b"abc").unwrap();
        assert_eq!(m.group(0), None);
    }

    #[test]
    fn group_ten_returns_none() {
        let m = Regex::compile(b"^\\(abc\\)$").find(b"abc").unwrap();
        assert_eq!(m.group(10), None);
    }

    // Backslash escaping

    #[test]
    fn escaped_dot_is_literal() {
        assert!(has_match(b"a\\.c", b"a.c"));
        assert!(!has_match(b"a\\.c", b"abc"));
    }

    #[test]
    fn escaped_star_is_literal() {
        assert!(has_match(b"a\\*", b"a*"));
        assert!(!has_match(b"a\\*", b"aaa"));
    }

    #[test]
    fn escaped_backslash_is_literal() {
        assert!(has_match(b"a\\\\b", b"a\\b"));
    }

    // ── Step 5 tests: backreferences ─────────────────────────

    #[test]
    fn backref_simple() {
        // \(abc\)\1 matches "abcabc"
        assert!(has_match(b"^\\(abc\\)\\1$", b"abcabc"));
    }

    #[test]
    fn backref_rejects_mismatch() {
        assert!(!has_match(b"^\\(abc\\)\\1$", b"abcdef"));
    }

    #[test]
    fn backref_with_dot() {
        // \(.\)\1 matches any repeated character
        assert!(has_match(b"^\\(.\\)\\1$", b"aa"));
        assert!(!has_match(b"^\\(.\\)\\1$", b"ab"));
    }

    #[test]
    fn backref_two_groups() {
        // \(a\)\(b\)\2\1 matches "abba"
        let m = Regex::compile(b"^\\(a\\)\\(b\\)\\2\\1$")
            .find(b"abba")
            .unwrap();
        assert_eq!(m.group(1), Some((0, 1)));
        assert_eq!(m.group(2), Some((1, 2)));
    }

    #[test]
    fn backref_to_uncaptured_fails() {
        // \1 with no group should fail to match
        assert!(!has_match(b"^\\1$", b"anything"));
    }

    #[test]
    fn backref_multi_char() {
        // \(hello\).*\1 matches "hello world hello"
        assert!(has_match(
            b"^\\(hello\\).*\\1$",
            b"hello world hello"
        ));
    }

    #[test]
    fn backref_multi_char_rejects() {
        assert!(!has_match(
            b"^\\(hello\\).*\\1$",
            b"hello world help"
        ));
    }

    #[test]
    fn backref_with_class() {
        // \([a-z]\)\1 finds a repeated lowercase letter
        assert!(has_match(b"\\([a-z]\\)\\1", b"xaabbx"));
        assert!(!has_match(b"^\\([a-z]\\)\\1$", b"ab"));
    }
}
