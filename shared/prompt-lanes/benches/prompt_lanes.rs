use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use prompt_lanes::{Adapter as _, MarkdownAdapter, ParsedPrompt, parse};

const SAMPLE_SIZE: usize = 20;
const PLAIN_PROMPT: &str = "refactor the prompt lane parser without changing its output";
const STRUCTURED_PROMPT: &str = "refactor prompt lanes / preserve authored goals / keep marker order stable /c parsing currently allocates token and bullet buffers /n retain the public ParsedPrompt contract /n preserve whitespace normalization /d parser and renderer tests remain green /d benchmarks show the cost of each stage";

fn prompt_lanes(criterion: &mut Criterion) {
    let dense_prompt = dense_prompt();
    let cases = [
        PromptCase::new("plain", PLAIN_PROMPT),
        PromptCase::new("structured", STRUCTURED_PROMPT),
        PromptCase::new("marker-dense", &dense_prompt),
    ];

    benchmark_parse(criterion, &cases);
    benchmark_render_markdown(criterion, &cases);
    benchmark_parse_and_render(criterion, &cases);
}

fn benchmark_parse(criterion: &mut Criterion, cases: &[PromptCase<'_>]) {
    let mut group = criterion.benchmark_group("prompt-lanes/parse");
    for case in cases {
        group.throughput(Throughput::Bytes(case.prompt.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.prompt,
            |bencher, prompt| {
                bencher.iter(|| black_box(parse(black_box(prompt))));
            },
        );
    }
    group.finish();
}

fn benchmark_render_markdown(criterion: &mut Criterion, cases: &[PromptCase<'_>]) {
    let parsed = cases
        .iter()
        .map(|case| ParsedCase {
            name: case.name,
            prompt: parse(case.prompt),
        })
        .collect::<Vec<_>>();
    let mut group = criterion.benchmark_group("prompt-lanes/render-markdown");
    for case in &parsed {
        let rendered_bytes = MarkdownAdapter.render(&case.prompt).len() as u64;
        group.throughput(Throughput::Bytes(rendered_bytes));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.prompt,
            |bencher, prompt| {
                bencher.iter(|| black_box(MarkdownAdapter.render(black_box(prompt))));
            },
        );
    }
    group.finish();
}

fn benchmark_parse_and_render(criterion: &mut Criterion, cases: &[PromptCase<'_>]) {
    let mut group = criterion.benchmark_group("prompt-lanes/parse-and-render");
    for case in cases {
        group.throughput(Throughput::Bytes(case.prompt.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.prompt,
            |bencher, prompt| {
                bencher.iter(|| black_box(parse_and_render(black_box(prompt))));
            },
        );
    }
    group.finish();
}

fn parse_and_render(prompt: &str) -> String {
    MarkdownAdapter.render(&parse(prompt))
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
    prompt: ParsedPrompt,
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = prompt_lanes
}
criterion_main!(benches);
