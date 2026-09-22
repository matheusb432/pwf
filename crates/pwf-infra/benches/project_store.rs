use std::{
    fmt::Debug,
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use pwf_application::ports::project_store::ProjectStore;
use pwf_infra::{database, project_store::SqliteProjectStore};
use pwf_models::project::{
    HomeDirectory, Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
    ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use pwf_wire::project::{GetProject, ProjectFields, ProjectStatusFilter, RenameProject};

struct Fixture {
    runtime: tokio::runtime::Runtime,
    pool: sqlx::SqlitePool,
    store: SqliteProjectStore,
    home: HomeDirectory,
    directory: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let runtime = require(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build(),
        );
        let directory = require(tempfile::tempdir());
        let pool = runtime.block_on(async {
            let path = directory.path().join("projects.sqlite3");
            let migrations = require(database::build_migration_pool(&path).await);
            require(database::migrate_database(&migrations).await);
            migrations.close().await;
            require(database::build_pool(&path).await)
        });
        Self {
            store: SqliteProjectStore::new(pool.clone()),
            pool,
            runtime,
            home: HomeDirectory::new(directory.path().to_path_buf()),
            directory,
        }
    }

    fn fields(&self, id: &str, title: &str) -> ProjectFields {
        ProjectFields {
            id: require(id.parse()),
            title: require(ProjectName::try_new(title)),
            source: Some(ProjectSource::new(
                ProjectSourceKind::Directory,
                require(ProjectSourceValue::try_new("/work/shared")),
            )),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                require(ProjectTasksPath::try_new(
                    self.directory
                        .path()
                        .join("tasks")
                        .join(id)
                        .to_string_lossy(),
                )),
            ),
            obsidian_vault: None,
            snapshot_enabled: true,
        }
    }

    fn add(&self, fields: ProjectFields) -> Project {
        require(
            self.runtime
                .block_on(self.store.add_project(fields, &self.home)),
        )
    }

    fn get(&self, id: &str) -> Project {
        require(
            self.runtime
                .block_on(self.store.get_project(GetProject::new(
                    project_id(id),
                    ProjectStatusFilter::IncludingPaused,
                ))),
        )
    }

    fn execute(&self, statement: &'static str) {
        require(
            self.runtime
                .block_on(sqlx::query(statement).execute(&self.pool)),
        );
    }

    fn measure_add(&self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            let fields = self.fields("FOO", "foo");
            let started = Instant::now();
            let project = self.add(fields);
            elapsed += started.elapsed();
            black_box(project);
            self.execute("DELETE FROM projects WHERE id = 'FOO'");
        }
        elapsed
    }

    fn measure_pause(&self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            self.execute("UPDATE projects SET paused_at = NULL WHERE id = 'FOO'");
            let started = Instant::now();
            let change = require(
                self.runtime
                    .block_on(self.store.pause_project(project_id("FOO"))),
            );
            elapsed += started.elapsed();
            black_box(change);
        }
        elapsed
    }

    fn measure_resume(&self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            self.execute("UPDATE projects SET paused_at = '2026-01-01T00:00:00Z' WHERE id = 'FOO'");
            let started = Instant::now();
            let change = require(
                self.runtime
                    .block_on(self.store.resume_project(project_id("FOO"), &self.home)),
            );
            elapsed += started.elapsed();
            black_box(change);
        }
        elapsed
    }

    fn measure_rename(&self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            self.execute(
                "UPDATE projects SET id = 'FOO', title = 'foo', tasks_path = replace(tasks_path, '/BAR', '/FOO') WHERE id = 'BAR'",
            );
            let expected = self.get("FOO");
            let command = RenameProject {
                current_id: project_id("FOO"),
                fields: self.fields("BAR", "bar"),
            };
            let started = Instant::now();
            let project = require(
                self.runtime
                    .block_on(self.store.rename_project(command, &expected, &self.home)),
            );
            elapsed += started.elapsed();
            black_box(project);
        }
        elapsed
    }
}

fn project_store(criterion: &mut Criterion) {
    eprintln!("project-store fixture_schema=1 sqlite_journal=wal cache=cold source=existing");

    let add = Fixture::new();
    add.add(add.fields("SRC", "source"));
    let added = add.add(add.fields("FOO", "foo"));
    assert_eq!(added.id, project_id("FOO"));
    add.execute("DELETE FROM projects WHERE id = 'FOO'");
    criterion.bench_function("project-store/add", |bencher| {
        bencher.iter_custom(|iterations| add.measure_add(iterations));
    });

    let pause = Fixture::new();
    pause.add(pause.fields("FOO", "foo"));
    let paused = require(
        pause
            .runtime
            .block_on(pause.store.pause_project(project_id("FOO"))),
    );
    assert!(paused.changed && paused.project.is_paused);
    criterion.bench_function("project-store/pause", |bencher| {
        bencher.iter_custom(|iterations| pause.measure_pause(iterations));
    });

    let resume = Fixture::new();
    resume.add(resume.fields("FOO", "foo"));
    resume.execute("UPDATE projects SET paused_at = '2026-01-01T00:00:00Z' WHERE id = 'FOO'");
    let resumed = require(
        resume
            .runtime
            .block_on(resume.store.resume_project(project_id("FOO"), &resume.home)),
    );
    assert!(resumed.changed && !resumed.project.is_paused);
    criterion.bench_function("project-store/resume", |bencher| {
        bencher.iter_custom(|iterations| resume.measure_resume(iterations));
    });

    let rename = Fixture::new();
    rename.add(rename.fields("FOO", "foo"));
    let renamed = require(rename.runtime.block_on(rename.store.rename_project(
        RenameProject {
            current_id: project_id("FOO"),
            fields: rename.fields("BAR", "bar"),
        },
        &rename.get("FOO"),
        &rename.home,
    )));
    assert_eq!(renamed.id, project_id("BAR"));
    criterion.bench_function("project-store/rename", |bencher| {
        bencher.iter_custom(|iterations| rename.measure_rename(iterations));
    });
}

fn project_id(raw: &str) -> ProjectId {
    require(raw.parse())
}

fn require<T>(result: Result<T, impl Debug>) -> T {
    result.unwrap_or_else(|error| {
        eprintln!("project store benchmark failed: {error:?}");
        std::process::exit(1);
    })
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = project_store
}
criterion_main!(benches);
