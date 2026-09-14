use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pwf_marker_sections::{
    Adapter as _, MarkdownAdapter, MarkerSectionConfiguration, MarkerSectionDefinition,
    ParsedMarkerSections, parse,
};

const SAMPLE_SIZE: usize = 20;
const PLAIN_BODY: &str = "refactor the body section parser without changing its output";
const STRUCTURED_BODY: &str = "refactor body sections / preserve authored goals / keep marker order stable /c parsing currently allocates token and bullet buffers /n retain the public ParsedMarkerSections contract /n preserve whitespace normalization /d parser and renderer tests remain green /d benchmarks show the cost of each stage";

fn pwf_marker_sections(criterion: &mut Criterion) {
    let configuration = configuration();
    let dense_body = dense_body();
    let cases = [
        BodyCase::new("plain", PLAIN_BODY),
        BodyCase::new("structured", STRUCTURED_BODY),
        BodyCase::new("marker-dense", &dense_body),
    ];

    benchmark_parse(criterion, &configuration, &cases);
    benchmark_render_markdown(criterion, &configuration, &cases);
    benchmark_parse_and_render(criterion, &configuration, &cases);
}

fn benchmark_parse(
    criterion: &mut Criterion,
    configuration: &MarkerSectionConfiguration<4>,
    cases: &[BodyCase<'_>],
) {
    let mut group = criterion.benchmark_group("pwf-marker-sections/parse");
    for case in cases {
        group.throughput(Throughput::Bytes(case.body.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.body,
            |bencher, body| {
                bencher.iter(|| black_box(parse(black_box(body), black_box(configuration))));
            },
        );
    }
    group.finish();
}

fn benchmark_render_markdown(
    criterion: &mut Criterion,
    configuration: &MarkerSectionConfiguration<4>,
    cases: &[BodyCase<'_>],
) {
    let parsed = cases
        .iter()
        .map(|case| ParsedCase {
            name: case.name,
            body: parse(case.body, configuration),
        })
        .collect::<Vec<_>>();
    let mut group = criterion.benchmark_group("pwf-marker-sections/render-markdown");
    for case in &parsed {
        let adapter = MarkdownAdapter::new(configuration);
        let rendered_bytes = adapter.render(&case.body).len() as u64;
        group.throughput(Throughput::Bytes(rendered_bytes));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.body,
            |bencher, body| {
                bencher.iter(|| black_box(adapter.render(black_box(body))));
            },
        );
    }
    group.finish();
}

fn benchmark_parse_and_render(
    criterion: &mut Criterion,
    configuration: &MarkerSectionConfiguration<4>,
    cases: &[BodyCase<'_>],
) {
    let mut group = criterion.benchmark_group("pwf-marker-sections/parse-and-render");
    for case in cases {
        benchmark_parse_and_render_case(&mut group, configuration, case);
    }
    group.finish();
}

fn benchmark_parse_and_render_case(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    configuration: &MarkerSectionConfiguration<4>,
    case: &BodyCase<'_>,
) {
    group.throughput(Throughput::Bytes(case.body.len() as u64));
    group.bench_with_input(
        BenchmarkId::from_parameter(case.name),
        &case.body,
        |bencher, body| {
            bencher.iter(|| black_box(parse_and_render(black_box(body), black_box(configuration))));
        },
    );
}

fn parse_and_render(body: &str, configuration: &MarkerSectionConfiguration<4>) -> String {
    MarkdownAdapter::new(configuration).render(&parse(body, configuration))
}

fn configuration() -> MarkerSectionConfiguration<4> {
    require_configuration(MarkerSectionConfiguration::try_new([
        require_section(MarkerSectionDefinition::try_new("/g", "Goals")),
        require_section(MarkerSectionDefinition::try_new("/c", "Context")),
        require_section(MarkerSectionDefinition::try_new("/n", "Constraints")),
        require_section(MarkerSectionDefinition::try_new("/d", "Done When")),
    ]))
}

fn require_section(
    result: Result<MarkerSectionDefinition, pwf_marker_sections::MarkerSectionDefinitionError>,
) -> MarkerSectionDefinition {
    result.unwrap_or_else(|error| benchmark_configuration_error(&error))
}

fn require_configuration(
    result: Result<
        MarkerSectionConfiguration<4>,
        pwf_marker_sections::MarkerSectionConfigurationError,
    >,
) -> MarkerSectionConfiguration<4> {
    result.unwrap_or_else(|error| benchmark_configuration_error(&error))
}

fn benchmark_configuration_error(error: &dyn std::fmt::Display) -> ! {
    eprintln!("invalid pwf-marker-sections benchmark configuration: {error}");
    std::process::exit(1)
}

fn dense_body() -> String {
    let markers = ["/g", "/c", "/n", "/d"];
    (0..64)
        .map(|index| {
            let marker = markers[index % markers.len()];
            format!("{marker} section item {index} retains meaningful authored text")
        })
        .fold(
            String::from("refactor dense body sections"),
            |mut body, section| {
                body.push(' ');
                body.push_str(&section);
                body
            },
        )
}

struct BodyCase<'body> {
    name: &'static str,
    body: &'body str,
}

impl<'body> BodyCase<'body> {
    const fn new(name: &'static str, body: &'body str) -> Self {
        Self { name, body }
    }
}

struct ParsedCase {
    name: &'static str,
    body: ParsedMarkerSections<4>,
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = pwf_marker_sections
}
criterion_main!(benches);
