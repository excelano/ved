// The text buffer that ved edits.
//
// Lines are 1-indexed externally, matching ed's address syntax: line
// 1 is the first line, $ is the last, and 0 is a sentinel meaning
// "before the first line" (which is what `a` on an empty buffer
// effectively writes against).
//
// The `current` field tracks ed's "current line" concept — the line
// that most commands default to when no address is given. After most
// operations current points at the last line touched.
//
// `modified` flips to true on any mutation and clears on a successful
// whole-buffer write. It's what `q` consults to decide whether to
// warn about unsaved changes.
//
// `filename` remembers the last filename used by `w`, so a bare `w`
// after `w foo.txt` writes back to foo.txt. Slice 11 will also set
// this when ved is invoked with a filename argument.

pub struct Buffer {
    lines: Vec<String>,
    current: usize, // 0 means empty buffer / before-first-line
    modified: bool,
    filename: Option<String>,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            current: 0,
            modified: false,
            filename: None,
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn current(&self) -> usize {
        self.current
    }

    /// Set the current line. Used by commands like `p` and `n` to
    /// match ed's behavior of leaving the current line at the end
    /// of whatever range they touched.
    pub fn set_current(&mut self, n: usize) {
        self.current = n;
    }

    /// Insert `line` after 1-indexed position `after`. `after = 0`
    /// means insert at the very start of the buffer (the case when
    /// you run `a` on an empty buffer). Updates current to the
    /// newly inserted line.
    pub fn append_after(&mut self, after: usize, line: String) {
        self.lines.insert(after, line);
        self.current = after + 1;
        self.modified = true;
    }

    /// Fetch a line by 1-indexed position. Returns None if the
    /// address is out of range.
    pub fn line(&self, n: usize) -> Option<&str> {
        if n == 0 || n > self.lines.len() {
            None
        } else {
            Some(self.lines[n - 1].as_str())
        }
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Called by `w` after a successful whole-buffer write. A
    /// partial-range write does NOT clear modified, since the
    /// buffer still differs from any single file on disk.
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    pub fn set_filename(&mut self, name: String) {
        self.filename = Some(name);
    }

    /// Replace the content of 1-indexed line `n`. Marks the buffer
    /// as modified. Panics if `n` is out of range.
    pub fn replace_line(&mut self, n: usize, content: String) {
        self.lines[n - 1] = content;
        self.modified = true;
    }

    /// Delete lines from `start` to `end` (1-indexed, inclusive).
    /// Updates current to the line after the deleted range, or the
    /// new last line, or 0 if the buffer is now empty.
    pub fn delete_range(&mut self, start: usize, end: usize) {
        self.lines.drain((start - 1)..end);
        self.modified = true;
        if self.lines.is_empty() {
            self.current = 0;
        } else if start <= self.lines.len() {
            self.current = start;
        } else {
            self.current = self.lines.len();
        }
    }
}
