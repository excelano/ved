// ed-style address parsing and resolution.
//
// Every ed command can be prefixed by an address or an address
// range. This module turns the front of a command string into a
// `Spec` (the parsed-but-unresolved form), and resolves a `Spec`
// against a `Buffer` to produce a concrete `Range` of 1-indexed
// line numbers.
//
// Address forms recognized in slice 3:
//   5         absolute line number
//   .         current line
//   $         last line
//   +3        current + 3
//   -1        current - 1
//   ,         shorthand for 1,$  (whole buffer)
//   ;         shorthand for .,$  (current to end)
//   2,4       a range from line 2 to line 4
//   .,+5      from current to current+5
//
// Search addresses (/foo/, ?foo?) wait for slice 5 when the regex
// engine arrives. Compound expressions like $-5 also wait.

use crate::buffer::Buffer;

pub enum Address {
    Number(usize),
    Current,
    Last,
    Offset(isize),
}

pub struct Spec {
    pub start: Option<Address>,
    pub end: Option<Address>,
}

pub struct Range {
    pub start: usize, // 1-indexed, inclusive
    pub end: usize,   // 1-indexed, inclusive
}

impl Spec {
    /// Parse an address spec from the front of `input`. Returns the
    /// spec and the remaining string (command letter + args).
    ///
    /// This does NOT validate that the addresses are in range —
    /// that's `resolve`'s job. It just tokenizes.
    pub fn parse(input: &str) -> Result<(Spec, &str), String> {
        // Handle the two shorthand forms first since they're a
        // single character standing in for a full range.
        let bytes = input.as_bytes();
        if let Some(&first) = bytes.first() {
            if first == b',' {
                return Ok((
                    Spec {
                        start: Some(Address::Number(1)),
                        end: Some(Address::Last),
                    },
                    &input[1..],
                ));
            }
            if first == b';' {
                return Ok((
                    Spec {
                        start: Some(Address::Current),
                        end: Some(Address::Last),
                    },
                    &input[1..],
                ));
            }
        }

        // Otherwise: parse one address, then optionally a `,` and
        // a second address.
        let (start, rest) = parse_one(input)?;
        if let Some(rest) = rest.strip_prefix(',') {
            let (end, rest) = parse_one(rest)?;
            // ed allows a comma with no second address, meaning
            // "end at the last line". e.g. `5,p` = `5,$p`. Most
            // people don't use this but it's part of the language.
            let end = end.or(Some(Address::Last));
            return Ok((Spec { start, end }, rest));
        }
        Ok((Spec { start, end: None }, rest))
    }

    /// True if no address was given at all. Used by commands like
    /// `w` to detect the "no spec, no buffer either" case where
    /// resolving would error but the command should still succeed
    /// (writing a 0-byte file from an empty buffer).
    pub fn is_empty(&self) -> bool {
        self.start.is_none() && self.end.is_none()
    }

    /// Resolve with current-line as the default for an omitted
    /// spec. Used by `p`, `n`, and most other commands.
    pub fn resolve(&self, buf: &Buffer) -> Result<Range, String> {
        self.resolve_with(buf, (Address::Current, Address::Current))
    }

    /// Resolve with the whole buffer (`1,$`) as the default for an
    /// omitted spec. Used by `w` — typing `w foo.txt` with no
    /// address writes the entire buffer, not just the current line.
    pub fn resolve_or_whole(&self, buf: &Buffer) -> Result<Range, String> {
        self.resolve_with(buf, (Address::Number(1), Address::Last))
    }

    fn resolve_with(
        &self,
        buf: &Buffer,
        default: (Address, Address),
    ) -> Result<Range, String> {
        if buf.is_empty() {
            return Err("invalid address".to_string());
        }

        let (start_addr, end_addr) = match (&self.start, &self.end) {
            (None, None) => default,
            (Some(s), None) => (clone_addr(s), clone_addr(s)),
            (Some(s), Some(e)) => (clone_addr(s), clone_addr(e)),
            (None, Some(_)) => unreachable!("parser never produces (None, Some)"),
        };

        let start = resolve_one(&start_addr, buf)?;
        let end = resolve_one(&end_addr, buf)?;

        if start == 0 || end == 0 || start > buf.len() || end > buf.len() {
            return Err("invalid address".to_string());
        }
        if start > end {
            return Err("invalid address".to_string());
        }
        Ok(Range { start, end })
    }
}

/// Parse a single address atom. Returns `(maybe_address, rest)`.
/// `Ok((None, rest))` means "no address here, the rest is the
/// command letter" — used by callers that need to distinguish an
/// empty address from a parse error.
fn parse_one(input: &str) -> Result<(Option<Address>, &str), String> {
    let bytes = input.as_bytes();
    let Some(&first) = bytes.first() else {
        return Ok((None, input));
    };
    match first {
        b'.' => Ok((Some(Address::Current), &input[1..])),
        b'$' => Ok((Some(Address::Last), &input[1..])),
        b'+' | b'-' => {
            let sign: isize = if first == b'+' { 1 } else { -1 };
            let (digits, rest) = take_digits(&input[1..]);
            // `+` alone (no digits) means `+1`, ditto `-`.
            let n: isize = if digits.is_empty() {
                1
            } else {
                digits
                    .parse::<isize>()
                    .map_err(|_| "invalid address".to_string())?
            };
            Ok((Some(Address::Offset(sign * n)), rest))
        }
        b'0'..=b'9' => {
            let (digits, rest) = take_digits(input);
            let n: usize = digits
                .parse::<usize>()
                .map_err(|_| "invalid address".to_string())?;
            Ok((Some(Address::Number(n)), rest))
        }
        _ => Ok((None, input)),
    }
}

/// Pull a run of ASCII digits off the front of `s`. Returns the
/// digit slice and the remainder.
fn take_digits(s: &str) -> (&str, &str) {
    let end = s
        .as_bytes()
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(s.len());
    s.split_at(end)
}

fn resolve_one(addr: &Address, buf: &Buffer) -> Result<usize, String> {
    match addr {
        Address::Number(n) => Ok(*n),
        Address::Current => Ok(buf.current()),
        Address::Last => Ok(buf.len()),
        Address::Offset(n) => {
            let base = buf.current() as isize;
            let result = base + n;
            if result < 1 {
                Err("invalid address".to_string())
            } else {
                Ok(result as usize)
            }
        }
    }
}

fn clone_addr(a: &Address) -> Address {
    match a {
        Address::Number(n) => Address::Number(*n),
        Address::Current => Address::Current,
        Address::Last => Address::Last,
        Address::Offset(n) => Address::Offset(*n),
    }
}
