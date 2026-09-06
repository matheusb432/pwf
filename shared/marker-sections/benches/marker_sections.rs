use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use marker_sections::{
    Adapter as _, LaneConfiguration, LaneDefinition, MarkdownAdapter, ParsedPrompt, parse,
};

const SAMPLE_SIZE: usize = 20;
const PLAIN_PROMPT: &str = "refactor the prompt lane parser without changing its output";
const STRUCTURED_PROMPT: &str = "refactor prompt lanes / preserve authored goals / keep marker order stable /c parsing currently allocates token and bullet buffers /n retain the public ParsedPrompt contract /n preserve whitespace normalization /d parser and renderer tests remain green /d benchmarks show the cost of each stage";

fn marker_sections(criterion: &mut Criterion) {
    let configuration = configuration();
    let dense_prompt = dense_prompt();
    let cases = [
        PromptCase::new("plain", PLAIN_PROMPT),
        PromptCase::new("structured", STRUCTURED_PROMPT),
        PromptCase::new("marker-dense", &dense_prompt),
    ];

    benchmark_parse(criterion, &configuration, &cases);
    benchmark_render_markdown(criterion, &configuration, &cases);
    benchmark_parse_and_render(criterion, &configuration, &cases);
}

fn benchmark_parse(
    criterion: &mut Criterion,
    configuration: &LaneConfiguration<4>,
    cases: &[PromptCase<'_>],
) {
    let mut group = criterion.benchmark_group("marker-sections/parse");
    for case in cases {
        group.throughput(Throughput::Bytes(case.prompt.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.prompt,
            |bencher, prompt| {
                bencher.iter(|| black_box(parse(black_box(prompt), black_box(configuration))));
            },
        );
    }
    group.finish();
}

fn benchmark_render_markdown(
    criterion: &mut Criterion,
    configuration: &LaneConfiguration<4>,
    cases: &[PromptCase<'_>],
) {
    let parsed = cases
        .iter()
        .map(|case| ParsedCase {
            name: case.name,
            prompt: parse(case.prompt, configuration),
        })
        .collect::<Vec<_>>();
    let mut group = criterion.benchmark_group("marker-sections/render-markdown");
    for case in &parsed {
        let adapter = MarkdownAdapter::new(configuration);
        let rendered_bytes = adapter.render(&case.prompt).len() as u64;
        group.throughput(Throughput::Bytes(rendered_bytes));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.prompt,
            |bencher, prompt| {
                bencher.iter(|| black_box(adapter.render(black_box(prompt))));
            },
        );
    }
    group.finish();
}

fn benchmark_parse_and_render(
    criterion: &mut Criterion,
    configuration: &LaneConfiguration<4>,
    cases: &[PromptCase<'_>],
) {
    let mut group = criterion.benchmark_group("marker-sections/parse-and-render");
    for case in cases {
        benchmark_parse_and_render_case(&mut group, configuration, case);
    }
    group.finish();
}

fn benchmark_parse_and_render_case(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    configuration: &LaneConfiguration<4>,
    case: &PromptCase<'_>,
) {
    group.throughput(Throughput::Bytes(case.prompt.len() as u64));
    group.bench_with_input(
        BenchmarkId::from_parameter(case.name),
        &case.prompt,
        |bencher, prompt| {
            bencher.iter(|| {
                black_box(parse_and_render(
                    black_box(prompt),
                    black_box(configuration),
                ))
            });
        },
    );
}

fn parse_and_render(prompt: &str, configuration: &LaneConfiguration<4>) -> String {
    MarkdownAdapter::new(configuration).render(&parse(prompt, configuration))
}

fn configuration() -> LaneConfiguration<4> {
    require_configuration(LaneConfiguration::try_new([
        require_lane(LaneDefinition::try_new("/g", "Goals")),
        require_lane(LaneDefinition::try_new("/c", "Context")),
        require_lane(LaneDefinition::try_new("/n", "Constraints")),
        require_lane(LaneDefinition::try_new("/d", "Done When")),
    ]))
}

fn require_lane(
    result: Result<LaneDefinition, marker_sections::LaneDefinitionError>,
) -> LaneDefinition {
    result.unwrap_or_else(|error| benchmark_configuration_error(&error))
}

fn require_configuration(
    result: Result<LaneConfiguration<4>, marker_sections::LaneConfigurationError>,
) -> LaneConfiguration<4> {
    result.unwrap_or_else(|error| benchmark_configuration_error(&error))
}

fn benchmark_configuration_error(error: &dyn std::fmt::Display) -> ! {
    eprintln!("invalid marker-sections benchmark configuration: {error}");
    std::process::exit(1)
}

fn dense_prompt() -> String {
    let markers = ["/g", "/c", "/n", "/d"];
    (0..64)
        .map(|index| {
            let marker = markers[index % markers.len()];
            format!("{marker} lane item {index} retains meaningful authored text")
        })
        .fold(
            String::from("refactor dense prompt lanes"),
            |mut prompt, lane| {
                prompt.push(' ');
                prompt.push_str(&lane);
                prompt
            },
        )
}

struct PromptCase<'prompt> {
    name: &'static str,
    prompt: &'prompt str,
}

impl<'prompt> PromptCase<'prompt> {
    const fn new(name: &'static str, prompt: &'prompt str) -> Self {
        Self { name, prompt }
    }
}

struct ParsedCase {
    name: &'static str,
    prompt: ParsedPrompt<4>,
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = marker_sections
}
criterion_main!(benches);
