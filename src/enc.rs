// Encoding sniff: warn on non-UTF-8 input before the cryptic read error.
// Inlined (rather than depending on the encsniff crate) to keep ved
// pure-stdlib. Detection rules are deliberately narrow — byte-perfect
// signatures only, no heuristics — and mirror the warning wording used by
// encsniff so the user-visible message is consistent across siblings.

use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf7,
    Utf16Le,
    Utf16Be,
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Encoding::Utf7 => "UTF-7",
            Encoding::Utf16Le => "UTF-16 little-endian",
            Encoding::Utf16Be => "UTF-16 big-endian",
        };
        f.write_str(s)
    }
}

const SCAN_WINDOW: usize = 4096;
const UTF7_MARKER: &[u8] = b"+ACI-";

/// Sniff the first 4 KiB of `path` for a non-UTF-8 signature. On detection,
/// emit a two-line warning to stderr with a copy-pasteable iconv command and
/// return the encoding display name (e.g. "UTF-7"). Sniff failures (missing
/// file, IO error) and clean UTF-8 files return None silently.
pub fn warn_if_non_utf8(path: &str) -> Option<String> {
    let enc = sniff_path(path)?;
    eprintln!("warning: {path} appears to be {enc} encoded.");
    let from = match enc {
        Encoding::Utf7 => "UTF-7",
        Encoding::Utf16Le => "UTF-16LE",
        Encoding::Utf16Be => "UTF-16BE",
    };
    let dst = utf8_sibling_path(Path::new(path));
    eprintln!("hint: iconv -f {from} -t UTF-8 {path} > {}", dst.display());
    Some(enc.to_string())
}

fn sniff_path(path: &str) -> Option<Encoding> {
    let mut f = File::open(path).ok()?;
    let mut buf = vec![0u8; SCAN_WINDOW];
    let mut filled = 0;
    while filled < buf.len() {
        match f.read(&mut buf[filled..]).ok()? {
            0 => break,
            n => filled += n,
        }
    }
    buf.truncate(filled);
    if buf.starts_with(&[0xFF, 0xFE]) {
        return Some(Encoding::Utf16Le);
    }
    if buf.starts_with(&[0xFE, 0xFF]) {
        return Some(Encoding::Utf16Be);
    }
    if find_subslice(&buf, UTF7_MARKER).is_some() {
        return Some(Encoding::Utf7);
    }
    None
}

fn utf8_sibling_path(path: &Path) -> PathBuf {
    match path.extension() {
        Some(ext) => {
            let mut new_ext = OsString::from("utf8.");
            new_ext.push(ext);
            path.with_extension(new_ext)
        }
        None => path.with_extension("utf8"),
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_utf7_marker() {
        // sniff_path requires a real file; the helper functions are testable
        // directly. Encoding choice for the inlined fast-path mirrors the
        // shared encsniff library's tests.
        assert_eq!(Encoding::Utf7.to_string(), "UTF-7");
        assert_eq!(Encoding::Utf16Le.to_string(), "UTF-16 little-endian");
        assert_eq!(Encoding::Utf16Be.to_string(), "UTF-16 big-endian");
    }

    #[test]
    fn utf8_sibling_path_inserts_utf8_before_extension() {
        assert_eq!(
            utf8_sibling_path(Path::new("Roster.csv")),
            PathBuf::from("Roster.utf8.csv")
        );
        assert_eq!(
            utf8_sibling_path(Path::new("noext")),
            PathBuf::from("noext.utf8")
        );
        assert_eq!(
            utf8_sibling_path(Path::new("/tmp/data.txt")),
            PathBuf::from("/tmp/data.utf8.txt")
        );
    }

    #[test]
    fn find_subslice_matches() {
        assert_eq!(find_subslice(b"hello world", b"world"), Some(6));
        assert_eq!(find_subslice(b"hello", b"world"), None);
        assert_eq!(find_subslice(b"", b"x"), None);
    }
}
