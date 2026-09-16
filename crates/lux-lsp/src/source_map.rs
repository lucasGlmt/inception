use lux_syntax::Span;
use tower_lsp::lsp_types::{Position, Range};

/// Central UTF-8 byte offset <-> LSP UTF-16 position mapper.
#[derive(Debug, Clone)]
pub struct SourceMap {
    line_starts: Vec<usize>,
    source_len: usize,
}

impl SourceMap {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }
        Self {
            line_starts,
            source_len: source.len(),
        }
    }

    pub fn position(&self, source: &str, offset: usize) -> Position {
        let offset = offset.min(self.source_len);
        let line = self.line_starts.partition_point(|start| *start <= offset) - 1;
        let start = self.line_starts[line];
        let character = source[start..offset]
            .encode_utf16()
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        Position::new(line as u32, character)
    }

    pub fn offset(&self, source: &str, position: Position) -> usize {
        let Some(&start) = self.line_starts.get(position.line as usize) else {
            return self.source_len;
        };
        let end = self
            .line_starts
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(self.source_len);
        let line = &source[start..end];
        let mut utf16 = 0u32;
        for (relative, ch) in line.char_indices() {
            if utf16 >= position.character {
                return start + relative;
            }
            let next = utf16 + ch.len_utf16() as u32;
            if next > position.character {
                return start + relative;
            }
            utf16 = next;
        }
        end
    }

    pub fn range(&self, source: &str, span: Span) -> Range {
        Range::new(
            self.position(source, span.start),
            self.position(source, span.end),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_utf8_bytes_to_utf16_positions() {
        let source = "// 💡 lumière\nscene main {}";
        let map = SourceMap::new(source);
        let scene = source.find("scene").unwrap();
        assert_eq!(map.position(source, scene), Position::new(1, 0));
        let after_emoji = source.find('💡').unwrap() + '💡'.len_utf8();
        assert_eq!(map.position(source, after_emoji), Position::new(0, 5));
        assert_eq!(map.offset(source, Position::new(0, 5)), after_emoji);
    }
}
