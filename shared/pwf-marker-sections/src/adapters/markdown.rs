use super::Adapter;
use crate::{configuration::MarkerSectionConfiguration, model::ParsedMarkerSections};

/// Renders configured sections as level-two Markdown sections and bullet lists.
pub struct MarkdownAdapter<'configuration, const N: usize> {
    configuration: &'configuration MarkerSectionConfiguration<N>,
}

impl<'configuration, const N: usize> MarkdownAdapter<'configuration, N> {
    #[must_use]
    pub const fn new(configuration: &'configuration MarkerSectionConfiguration<N>) -> Self {
        Self { configuration }
    }
}

impl<const N: usize> Adapter<N> for MarkdownAdapter<'_, N> {
    fn render(&self, parsed: &ParsedMarkerSections<N>) -> String {
        let capacity = rendered_length(self.configuration, parsed);
        let mut output = String::with_capacity(capacity);
        for (index, (section, items)) in self
            .configuration
            .sections()
            .iter()
            .zip(parsed.section_items())
            .enumerate()
            .filter(|(index, (_, items))| {
                *index == 0 || parsed.section_presence[*index] || !items.is_empty()
            })
        {
            append_section(&mut output, index, section.header(), items);
        }
        output
    }
}

fn append_section(output: &mut String, index: usize, header: &str, items: &[String]) {
    if index != 0 {
        output.push_str("\n\n");
    }
    output.push_str("## ");
    output.push_str(header);
    output.push('\n');
    for item in items {
        output.push_str("\n- ");
        output.push_str(item);
    }
}

fn rendered_length<const N: usize>(
    configuration: &MarkerSectionConfiguration<N>,
    parsed: &ParsedMarkerSections<N>,
) -> usize {
    configuration
        .sections()
        .iter()
        .zip(parsed.section_items())
        .enumerate()
        .filter(|(index, (_, items))| {
            *index == 0 || parsed.section_presence[*index] || !items.is_empty()
        })
        .map(|(index, (section, items))| {
            let section_separator = usize::from(index != 0) * 2;
            section_separator
                + "## ".len()
                + section.header().len()
                + 1
                + items
                    .iter()
                    .map(|item| "\n- ".len() + item.len())
                    .sum::<usize>()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MarkerSectionDefinition, parse};

    const S: &str = "\n\n";

    fn configuration() -> MarkerSectionConfiguration<4> {
        MarkerSectionConfiguration::try_new([
            MarkerSectionDefinition::try_new("/g", "Goals").unwrap(),
            MarkerSectionDefinition::try_new("/c", "Context").unwrap(),
            MarkerSectionDefinition::try_new("/n", "Constraints").unwrap(),
            MarkerSectionDefinition::try_new("/d", "Done When").unwrap(),
        ])
        .unwrap()
    }

    #[test]
    fn configured_headers_render_supported_sections_as_bullets() {
        let configuration = configuration();
        let body = "fix rich body parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        assert_eq!(
            MarkdownAdapter::new(&configuration).render(&parse(body, &configuration)),
            format!(
                "## Goals{S}- preserve ampersands in prose\n- keep code intact{S}## Context{S}- current add splits on ampersand{S}## Constraints{S}- no parser crate{S}## Done When{S}- tests cover add and update"
            )
        );
    }

    #[test]
    fn runtime_headers_replace_compile_time_defaults() {
        let configuration = MarkerSectionConfiguration::try_new([
            MarkerSectionDefinition::try_new("/o", "Objectives").unwrap(),
            MarkerSectionDefinition::try_new("/b", "Background").unwrap(),
        ])
        .unwrap();
        let parsed = parse("title / first /b second", &configuration);
        assert_eq!(
            MarkdownAdapter::new(&configuration).render(&parsed),
            "## Objectives\n\n- first\n\n## Background\n\n- second"
        );
    }
}
