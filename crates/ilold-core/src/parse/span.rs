use crate::model::common::SourceSpan;

pub struct LineIndex {
    file_index: usize,
    line_offsets: Vec<usize>,
}

impl LineIndex {
    pub fn new(file_index: usize, src: &str) -> Self {
        let mut line_offsets = vec![0usize];
        for (i, ch) in src.char_indices() {
            if ch == '\n' {
                line_offsets.push(i + 1);
            }
        }
        Self { file_index, line_offsets }
    }

    fn line_col(&self, offset: usize) -> (u32, u32) {
        let line = self.line_offsets.partition_point(|&o| o <= offset).saturating_sub(1);
        let col = offset.saturating_sub(self.line_offsets.get(line).copied().unwrap_or(0));
        ((line + 1) as u32, (col + 1) as u32)
    }

    pub fn span(&self, start: usize, end: usize) -> SourceSpan {
        let (start_line, start_col) = self.line_col(start);
        let (end_line, end_col) = self.line_col(end);
        SourceSpan { file_index: self.file_index, start_line, start_col, end_line, end_col }
    }
}
