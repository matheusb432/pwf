use super::Adapter;
use crate::{configuration::LaneConfiguration, model::ParsedPrompt};

/// Renders configured lanes as level-two Markdown sections and bullet lists.
pub struct MarkdownAdapter<'configuration, const N: usize> {
    configuration: &'configuration LaneConfiguration<N>,
}

impl<'configuration, const N: usize> MarkdownAdapter<'configuration, N> {
    #[must_use]
    pub const fn new(configuration: &'configuration LaneConfiguration<N>) -> Self {
        Self { configuration }
    }
}

impl<const N: usize> Adapter<N> for MarkdownAdapter<'_, N> {
    fn render(&self, parsed: &ParsedPrompt<N>) -> String {
        if parsed.lane_items().iter().all(Vec::is_empty) {
            return render_empty_lanes(self.configuration);
        }
        let capacity = rendered_length(self.configuration, parsed);
        let mut output = String::with_capacity(capacity);
        for (index, (lane, items)) in self
            .configuration
            .lanes()
            .iter()
            .zip(parsed.lane_items())
            .enumerate()
            .filter(|(index, (_, items))| *index == 0 || !items.is_empty())
        {
            append_section(&mut output, index, lane.header(), items);
        }
        output
    }
}

fn render_empty_lanes<const N: usize>(configuration: &LaneConfiguration<N>) -> String {
    let header = configuration.lanes()[0].header();
    let mut output = String::with_capacity("## ".len() + header.len() + 1);
    output.push_str("## ");
    output.push_str(header);
    output.push('\n');
    output
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
    configuration: &LaneConfiguration<N>,
    parsed: &ParsedPrompt<N>,
) -> usize {
    configuration
        .lanes()
        .iter()
        .zip(parsed.lane_items())
        .enumerate()
        .filter(|(index, (_, items))| *index == 0 || !items.is_empty())
        .map(|(index, (lane, items))| {
            let section_separator = usize::from(index != 0) * 2;
            section_separator
                + "## ".len()
                + lane.header().len()
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
    use crate::{LaneDefinition, parse};

    const S: &str = "\n\n";

    fn configuration() -> LaneConfiguration<4> {
        LaneConfiguration::try_new([
            LaneDefinition::try_new("/g", "Goals").unwrap(),
            LaneDefinition::try_new("/c", "Context").unwrap(),
            LaneDefinition::try_new("/n", "Constraints").unwrap(),
            LaneDefinition::try_new("/d", "Done When").unwrap(),
        ])
        .unwrap()
    }

    #[test]
    fn configured_headers_render_supported_lanes_as_bullets() {
        let configuration = configuration();
        let prompt = "fix rich prompt parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        assert_eq!(
            MarkdownAdapter::new(&configuration).render(&parse(prompt, &configuration)),
            format!(
                "## Goals{S}- preserve ampersands in prose\n- keep code intact{S}## Context{S}- current add splits on ampersand{S}## Constraints{S}- no parser crate{S}## Done When{S}- tests cover add and update"
            )
        );
    }

    #[test]
    fn runtime_headers_replace_compile_time_defaults() {
        let configuration = LaneConfiguration::try_new([
            LaneDefinition::try_new("/o", "Objectives").unwrap(),
            LaneDefinition::try_new("/b", "Background").unwrap(),
        ])
        .unwrap();
        let parsed = parse("title / first /b second", &configuration);
        assert_eq!(
            MarkdownAdapter::new(&configuration).render(&parsed),
            "## Objectives\n\n- first\n\n## Background\n\n- second"
        );
    }
}
