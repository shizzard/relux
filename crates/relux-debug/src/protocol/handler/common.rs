use relux_core::table::SourceFile;

pub fn start_line(sf: &SourceFile, byte_start: usize) -> usize {
    sf.line_at(byte_start)
}

/// `endLine` per `00-common.md`: line *after* the last line (1-based, exclusive).
pub fn end_line(sf: &SourceFile, byte_end: usize) -> usize {
    if byte_end == 0 {
        return 1;
    }
    sf.line_at(byte_end - 1) + 1
}
