// ved's BRE engine — bottom-up.
//
// Slice 5a starts as a translation of the matcher from Brian
// Kernighan's and Rob Pike's "Practice of Programming" / "Beautiful
// Code" chapter: ~30 lines of recursive descent that handles
// literal, `.`, `*`, `^`, and `$` against a byte string. Three
// functions, no compile step, the pattern itself is the program.
//
// We'll extend this bottom-up in later steps: bracket expressions
// first, then a struct refactor to carry capture state, then
// `\(...\)` groups, then `\1`-`\9` backreferences. Each extension
// keeps the three-function shape and adds branches or generalizes
// match_star.
//
// Three translation choices worth calling out up front:
//   * Pike's C uses `char *` and advances pointers with `regexp+1`.
//     We use `&[u8]` slices and `&pattern[1..]` — mechanically the
//     same, safer at the language level.
//   * Pike reads past the end of the string freely because C strings
//     are null-terminated: `regexp[1]` on a one-char pattern reads
//     the '\0'. Rust slices panic on out-of-bounds access, so we
//     guard each lookahead with a length check.
//   * Pike's function is named `match`, which is a Rust keyword.
//     Ours is `match_any`.

/// Search for `pattern` anywhere in `text`. Returns true on the
/// first match. Honors a leading `^` anchor by only trying at
/// position 0.
pub fn match_any(pattern: &[u8], mut text: &[u8]) -> bool {
    if !pattern.is_empty() && pattern[0] == b'^' {
        return match_here(&pattern[1..], text);
    }
    // Try each starting position, INCLUDING the empty tail, so that
    // `$` against `""` still gets a chance to match.
    loop {
        if match_here(pattern, text) {
            return true;
        }
        if text.is_empty() {
            return false;
        }
        text = &text[1..];
    }
}

/// Search for `pattern` at the beginning of `text`. The recursive
/// workhorse: peels one element off the pattern and recurses on the
/// tail.
fn match_here(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return true;
    }
    // Lookahead: if the next element is `*`, hand off to match_star.
    if pattern.len() >= 2 && pattern[1] == b'*' {
        return match_star(pattern[0], &pattern[2..], text);
    }
    // `$` only anchors when it's the last character of the pattern.
    // Everywhere else it's a literal, which is what BRE actually
    // specifies.
    if pattern[0] == b'$' && pattern.len() == 1 {
        return text.is_empty();
    }
    // Otherwise match one character (literal or `.`) and recurse.
    if !text.is_empty() && (pattern[0] == b'.' || pattern[0] == text[0]) {
        return match_here(&pattern[1..], &text[1..]);
    }
    false
}

/// Search for `c*pattern` at the beginning of `text`. Tries zero
/// matches of `c` first, then one, then two, and so on, until
/// either the tail pattern matches or `c` stops consuming.
fn match_star(c: u8, pattern: &[u8], mut text: &[u8]) -> bool {
    loop {
        if match_here(pattern, text) {
            return true;
        }
        if text.is_empty() {
            return false;
        }
        if text[0] != c && c != b'.' {
            return false;
        }
        text = &text[1..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Literal matching

    #[test]
    fn literal_exact_match() {
        assert!(match_any(b"abc", b"abc"));
    }

    #[test]
    fn literal_match_in_middle() {
        assert!(match_any(b"abc", b"xabcy"));
    }

    #[test]
    fn literal_no_match() {
        assert!(!match_any(b"abc", b"xyz"));
    }

    #[test]
    fn literal_pattern_longer_than_text() {
        assert!(!match_any(b"abc", b"ab"));
    }

    #[test]
    fn empty_pattern_always_matches() {
        assert!(match_any(b"", b"hello"));
    }

    // Dot wildcard

    #[test]
    fn dot_matches_letter() {
        assert!(match_any(b"a.c", b"abc"));
    }

    #[test]
    fn dot_matches_underscore() {
        assert!(match_any(b"a.c", b"a_c"));
    }

    #[test]
    fn dot_requires_a_character() {
        assert!(!match_any(b"a.c", b"ac"));
    }

    // Star repetition

    #[test]
    fn star_zero_matches() {
        assert!(match_any(b"a*b", b"b"));
    }

    #[test]
    fn star_many_matches() {
        assert!(match_any(b"a*b", b"aaab"));
    }

    #[test]
    fn star_combined_with_slide() {
        // Exercises both the outer slide and match_star's zero case:
        // at position 0, `a*b` fails because `xb` starts with x.
        // We slide to position 1 and match `b` with zero a's.
        assert!(match_any(b"a*b", b"xb"));
    }

    #[test]
    fn dot_star_matches_anything() {
        assert!(match_any(b".*", b"anything"));
    }

    // Anchors

    #[test]
    fn caret_anchors_at_start() {
        assert!(match_any(b"^abc", b"abc"));
    }

    #[test]
    fn caret_refuses_to_slide() {
        assert!(!match_any(b"^abc", b"xabc"));
    }

    #[test]
    fn dollar_anchors_at_end() {
        assert!(match_any(b"abc$", b"abc"));
    }

    #[test]
    fn dollar_refuses_tail() {
        assert!(!match_any(b"abc$", b"abcx"));
    }

    #[test]
    fn both_anchors() {
        assert!(match_any(b"^abc$", b"abc"));
    }

    #[test]
    fn empty_anchored_matches_empty() {
        assert!(match_any(b"^$", b""));
    }

    #[test]
    fn empty_anchored_rejects_nonempty() {
        assert!(!match_any(b"^$", b"x"));
    }

    // Tricky edge cases

    #[test]
    fn dollar_alone_finds_end() {
        // $ by itself slides through the whole text and matches at
        // the very end when text finally becomes empty.
        assert!(match_any(b"$", b"anything"));
    }

    #[test]
    fn anchored_star_on_empty() {
        assert!(match_any(b"^a*$", b""));
    }

    #[test]
    fn anchored_star_many() {
        assert!(match_any(b"^a*$", b"aaa"));
    }

    #[test]
    fn anchored_star_rejects_wrong_char() {
        assert!(!match_any(b"^a*$", b"aab"));
    }
}
