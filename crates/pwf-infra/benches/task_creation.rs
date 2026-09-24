use std::{
    fmt::Debug,
    fs,
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use pwf_application::task::add_task;
use pwf_infra::{
    clock::LocalClock, database, obsidian::ObsidianStore, project_store::SqliteProjectStore,
    user_settings::TomlSettingsStore,
};
use pwf_models::project::HomeDirectory;
use pwf_wire::task::{AddTask, AddTaskBody};

struct Fixture {
    runtime: tokio::runtime::Runtime,
    pool: sqlx::SqlitePool,
    task_count: usize,
    _directory: tempfile::TempDir,
    tasks: std::path::PathBuf,
    store: ObsidianStore,
    projects: SqliteProjectStore,
    settings: TomlSettingsStore,
}

impl Fixture {
    fn new(task_count: usize) -> Self {
        let runtime = require(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build(),
        );
        let directory = require(tempfile::tempdir());
        let tasks = directory.path().join("tasks");
        require(fs::create_dir(&tasks));
        for number in 1..=task_count {
            require(fs::write(
                tasks.join(format!("FOO-{number:04}.md")),
                format!(
                    "---\nid: FOO-{number:04}\ntitle: Existing task\nstatus: active\ncreated_at: 2026-09-13T00:00:00Z\n---\n\nDo the work.\n"
                ),
            ));
        }
        let pool = runtime.block_on(async {
            let path = directory.path().join("projects.sqlite3");
            let migrations = require(database::build_migration_pool(&path).await);
            require(database::migrate_database(&migrations).await);
            migrations.close().await;
            let pool = require(database::build_pool(&path).await);
            require(sqlx::query("INSERT INTO projects (id, title, tasks_kind, tasks_path) VALUES ('FOO', 'foo', 'directory', ?)")
                .bind(tasks.to_string_lossy().as_ref()).execute(&pool).await);
            pool
        });
        let store =
            ObsidianStore::with_watched_tasks(HomeDirectory::new(directory.path().to_path_buf()));
        Self {
            pool: pool.clone(),
            task_count,
            runtime,
            _directory: directory,
            tasks,
            store,
            projects: SqliteProjectStore::new(pool.clone()),
            settings: TomlSettingsStore::new(None),
        }
    }

    fn command() -> AddTask {
        AddTask::new(
            require("FOO".parse::<pwf_models::project::ProjectId>()),
            AddTaskBody::from_shorthand(
                "Create a benchmark task /g Measure the complete application operation",
            ),
        )
    }

    fn create(&self, command: AddTask) -> pwf_models::task::TaskId {
        require(self.runtime.block_on(add_task::execute(
            command,
            &self.store,
            &self.projects,
            &LocalClock,
            &self.settings,
        )))
        .outcome
    }

    fn remove(&self, id: &pwf_models::task::TaskId) {
        require(fs::remove_file(self.tasks.join(format!("{id}.md"))));
    }

    fn measure(&self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        for iteration in 0..iterations {
            if iteration % 1000 == 0 {
                // Reuse the disposable fixture without exhausting the four-digit ID space.
                require(
                    self.runtime.block_on(
                        sqlx::query("UPDATE projects SET last_task_number = ? WHERE id = 'FOO'")
                            .bind(require(i64::try_from(self.task_count)))
                            .execute(&self.pool),
                    ),
                );
            }
            let command = Fixture::command();
            let started = Instant::now();
            let id = black_box(self.create(command));
            elapsed += started.elapsed();
            self.remove(&id);
        }
        elapsed
    }

    fn validate(&self, task_count: usize) {
        let id = self.create(Self::command());
        assert_eq!(id.number() as usize, task_count + 1);
        let source = require(fs::read_to_string(self.tasks.join(format!("{id}.md"))));
        assert!(source.contains("Measure the complete application operation"));
        self.remove(&id);
    }
}

fn task_creation(criterion: &mut Criterion) {
    eprintln!(
        "task-creation fixture_schema=2 sqlite_journal=wal filesystem_cache=warm application_state=primed"
    );
    let mut group = criterion.benchmark_group("task-creation");
    for count in [0, 100, 1000] {
        let fixture = Fixture::new(count);
        fixture.validate(count);
        let allocations = allocation_counter::measure(|| {
            let id = black_box(fixture.create(Fixture::command()));
            fixture.remove(&id);
        });
        eprintln!(
            "allocations task-creation/{count} including request construction and cleanup {allocations:?}"
        );
        group.bench_function(BenchmarkId::from_parameter(count), |bencher| {
            bencher.iter_custom(|iterations| fixture.measure(iterations));
        });
    }
    group.finish();
}

fn require<T>(result: Result<T, impl Debug>) -> T {
    result.unwrap_or_else(|error| {
        eprintln!("task creation benchmark failed: {error:?}");
        std::process::exit(1);
    })
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));
    targets = task_creation
}
criterion_main!(benches);
