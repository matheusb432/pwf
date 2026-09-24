use std::fmt::Write as _;

use super::Adapter;
use crate::{
    configuration::{MarkerSectionConfiguration, MarkerSectionDefinition, MarkerSectionItemStyle},
    model::ParsedMarkerSections,
};

/// Renders configured sections as ATX Markdown sections with each section's item style.
pub struct MarkdownAdapter<'configuration> {
    configuration: &'configuration MarkerSectionConfiguration,
}

impl<'configuration> MarkdownAdapter<'configuration> {
    #[must_use]
    pub const fn new(configuration: &'configuration MarkerSectionConfiguration) -> Self {
        Self { configuration }
    }

    fn rendered_sections<'parsed>(
        &self,
        parsed: &'parsed ParsedMarkerSections,
    ) -> impl Iterator<
        Item = (
            usize,
            &'configuration MarkerSectionDefinition,
            &'parsed [String],
        ),
    > {
        self.configuration
            .sections()
            .iter()
            .zip(parsed.section_items())
            .enumerate()
            .filter(|(index, (_, items))| {
                *index == 0 || parsed.section_presence[*index] || !items.is_empty()
            })
            .map(|(index, (section, items))| (index, section, items.as_slice()))
    }
}

impl Adapter for MarkdownAdapter<'_> {
    fn render(&self, parsed: &ParsedMarkerSections) -> String {
        let capacity = self
            .rendered_sections(parsed)
            .map(|(index, section, items)| rendered_section_length(index, section, items))
            .sum();
        let mut output = String::with_capacity(capacity);
        for (index, section, items) in self.rendered_sections(parsed) {
            append_section(&mut output, index, section, items);
        }
        output
    }
}

fn append_section(
    output: &mut String,
    index: usize,
    section: &MarkerSectionDefinition,
    items: &[String],
) {
    if index != 0 {
        output.push_str("\n\n");
    }
    for _ in 0..section.heading_level().get() {
        output.push('#');
    }
    output.push(' ');
    output.push_str(section.header());
    output.push('\n');
    for (item_index, item) in items.iter().enumerate() {
        match section.item_style() {
            MarkerSectionItemStyle::Bullet => output.push_str("\n- "),
            MarkerSectionItemStyle::Numbered => {
                let _ = write!(output, "\n{}. ", item_index + 1);
            }
            MarkerSectionItemStyle::Paragraph => {
                output.push_str(if item_index == 0 { "\n" } else { "\n\n" });
            }
        }
        output.push_str(item);
    }
}

fn rendered_section_length(
    index: usize,
    section: &MarkerSectionDefinition,
    items: &[String],
) -> usize {
    let section_separator = usize::from(index != 0) * 2;
    let heading = usize::from(section.heading_level().get()) + " ".len() + section.header().len();
    let item_prefixes = (1..=items.len())
        .map(|number| match section.item_style() {
            MarkerSectionItemStyle::Bullet => "\n- ".len(),
            MarkerSectionItemStyle::Numbered => "\n. ".len() + decimal_digit_count(number),
            MarkerSectionItemStyle::Paragraph => {
                if number == 1 {
                    "\n".len()
                } else {
                    "\n\n".len()
                }
            }
        })
        .sum::<usize>();
    section_separator + heading + 1 + item_prefixes + items.iter().map(String::len).sum::<usize>()
}

const fn decimal_digit_count(number: usize) -> usize {
    number.ilog10() as usize + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MarkerSectionDefinition, MarkerSectionHeadingLevel, parse};

    const S: &str = "\n\n";

    fn definition(
        marker: &str,
        header: &str,
        heading_level: u8,
        item_style: MarkerSectionItemStyle,
    ) -> MarkerSectionDefinition {
        MarkerSectionDefinition::try_new(
            marker,
            header,
            MarkerSectionHeadingLevel::try_new(heading_level).unwrap(),
            item_style,
        )
        .unwrap()
    }

    fn configuration() -> MarkerSectionConfiguration {
        MarkerSectionConfiguration::try_new(vec![
            definition("/g", "Goals", 2, MarkerSectionItemStyle::Bullet),
            definition("/c", "Context", 2, MarkerSectionItemStyle::Bullet),
            definition("/n", "Constraints", 2, MarkerSectionItemStyle::Bullet),
            definition("/d", "Done When", 2, MarkerSectionItemStyle::Bullet),
        ])
        .unwrap()
    }

    fn render(body: &str, configuration: &MarkerSectionConfiguration) -> String {
        let rendered = MarkdownAdapter::new(configuration).render(&parse(body, configuration));
        let capacity = MarkdownAdapter::new(configuration)
            .rendered_sections(&parse(body, configuration))
            .map(|(index, section, items)| rendered_section_length(index, section, items))
            .sum::<usize>();
        assert_eq!(rendered.len(), capacity);
        rendered
    }

    #[test]
    fn configured_headers_render_supported_sections_as_bullets() {
        let body = "fix rich body parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        assert_eq!(
            render(body, &configuration()),
            format!(
                "## Goals{S}- preserve ampersands in prose\n- keep code intact{S}## Context{S}- current add splits on ampersand{S}## Constraints{S}- no parser crate{S}## Done When{S}- tests cover add and update"
            )
        );
    }

    #[test]
    fn runtime_headers_replace_compile_time_defaults() {
        let configuration = MarkerSectionConfiguration::try_new(vec![
            definition("/o", "Objectives", 2, MarkerSectionItemStyle::Bullet),
            definition("/b", "Background", 2, MarkerSectionItemStyle::Bullet),
        ])
        .unwrap();
        assert_eq!(
            render("title / first /b second", &configuration),
            "## Objectives\n\n- first\n\n## Background\n\n- second"
        );
    }

    #[test]
    fn sections_render_their_own_heading_level_and_item_style() {
        let configuration = MarkerSectionConfiguration::try_new(vec![
            definition("/g", "Goals", 3, MarkerSectionItemStyle::Numbered),
            definition("/c", "Context", 4, MarkerSectionItemStyle::Paragraph),
            definition("/n", "Constraints", 1, MarkerSectionItemStyle::Bullet),
        ])
        .unwrap();
        let goals = (1..=10)
            .map(|number| format!("goal {number}"))
            .collect::<Vec<_>>()
            .join(" / ");
        assert_eq!(
            render(
                &format!("title / {goals} /c first context /c second context /n keep"),
                &configuration
            ),
            format!(
                "### Goals{S}1. goal 1\n2. goal 2\n3. goal 3\n4. goal 4\n5. goal 5\n6. goal 6\n7. goal 7\n8. goal 8\n9. goal 9\n10. goal 10{S}#### Context{S}first context{S}second context{S}# Constraints{S}- keep"
            )
        );
    }
}
