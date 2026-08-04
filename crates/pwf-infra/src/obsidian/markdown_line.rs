#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MarkdownLine<'a> {
    pub start: usize,
    pub content_end: usize,
    pub end: usize,
    pub content: &'a str,
    pub text: &'a str,
    pub newline: &'static str,
}

pub(super) fn lines(source: &str) -> impl Iterator<Item = MarkdownLine<'_>> {
    let mut offset = 0;
    source.split_inclusive('\n').map(move |raw| {
        let start = offset;
        let end = start + raw.len();
        offset = end;
        let content = raw.strip_suffix('\n').unwrap_or(raw);
        let text = content.strip_suffix('\r').unwrap_or(content);
        let newline = if end == start + content.len() {
            ""
        } else if content.ends_with('\r') {
            "\r\n"
        } else {
            "\n"
        };
        MarkdownLine {
            start,
            content_end: start + content.len(),
            end,
            content,
            text,
            newline,
        }
    })
}

pub(super) fn find(
    source: &str,
    start: usize,
    predicate: impl Fn(&str) -> bool,
) -> Option<MarkdownLine<'_>> {
    lines(source)
        .filter(|line| line.start >= start)
        .find(|line| predicate(line.content))
}

#[cfg(test)]
mod tests {
    use super::lines;

    #[test]
    fn exposes_byte_ranges_and_newline_style() {
        let source = "one\r\ntwo\nthree";
        let lines = lines(source).collect::<Vec<_>>();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].start..lines[0].content_end, 0..4);
        assert_eq!(lines[0].text, "one");
        assert_eq!(lines[0].newline, "\r\n");
        assert_eq!(lines[1].start..lines[1].content_end, 5..8);
        assert_eq!(lines[1].newline, "\n");
        assert_eq!(lines[2].start..lines[2].content_end, 9..14);
        assert_eq!(lines[2].newline, "");
    }
}
