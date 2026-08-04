#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Checkbox {
    Open,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TaskLink<'a> {
    pub indentation: &'a str,
    pub checkbox: Option<Checkbox>,
    pub id: &'a str,
}

pub(super) fn parse(line: &str) -> Option<TaskLink<'_>> {
    let remainder = line.trim_start();
    let indentation = &line[..line.len() - remainder.len()];
    let mut remainder = remainder.strip_prefix('-')?.trim_start();
    let checkbox = if let Some(rest) = remainder.strip_prefix("[ ]") {
        remainder = rest.trim_start();
        Some(Checkbox::Open)
    } else if let Some(rest) = remainder
        .strip_prefix("[x]")
        .or_else(|| remainder.strip_prefix("[X]"))
    {
        remainder = rest.trim_start();
        Some(Checkbox::Done)
    } else {
        None
    };
    let wikilink = remainder.strip_prefix("[[")?;
    let closing = wikilink.find("]]")?;
    let target = &wikilink[..closing];
    if target.contains(']') {
        return None;
    }
    let id = target.split_once('|').map_or(target, |(id, _)| id);
    (!id.is_empty()).then_some(TaskLink {
        indentation,
        checkbox,
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::{Checkbox, TaskLink, parse};

    #[test]
    fn parses_bare_open_and_done_links_with_aliases() {
        assert_eq!(
            parse("- [[PWF-NOTE-0001]]"),
            Some(TaskLink {
                indentation: "",
                checkbox: None,
                id: "PWF-NOTE-0001",
            })
        );
        assert_eq!(
            parse("  - [ ] [[PWF-0001|task]]"),
            Some(TaskLink {
                indentation: "  ",
                checkbox: Some(Checkbox::Open),
                id: "PWF-0001",
            })
        );
        assert_eq!(
            parse("\t- [X] [[PWF-0002]]").unwrap().checkbox,
            Some(Checkbox::Done)
        );
    }
}
