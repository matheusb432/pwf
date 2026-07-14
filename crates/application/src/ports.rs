use pwf_domain::pending_work::{AddedItem, OpenItem, ProjectName, RemovedItem, Tags, UpdatedItem};

pub trait PendingWorkReadStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn open_items_for_project(&self, project: &ProjectName) -> Result<Vec<OpenItem>, Self::Error>;
    fn all_open_items(&self) -> Result<Vec<OpenItem>, Self::Error>;
    fn open_item(&self, id: &str) -> Result<OpenItem, Self::Error>;
}

pub trait PendingWorkResolveStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn resolve_item(&self, id: &str, show: bool) -> Result<ResolvePendingWorkOutput, Self::Error>;
}

pub trait PendingWorkWriteStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn add_item(&self, spec: AddItemSpec) -> Result<AddedItem, Self::Error>;
    fn update_item(&self, spec: UpdateItemSpec) -> Result<UpdatedItem, Self::Error>;
    fn remove_item(&self, id: &str) -> Result<RemovedItem, Self::Error>;
    fn complete_item(&self, spec: CompleteItemSpec) -> Result<ClosedItem, Self::Error>;
    fn cancel_item(&self, spec: CancelItemSpec) -> Result<ClosedItem, Self::Error>;
    fn reopen_item(&self, id: &str) -> Result<ReopenedItem, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddItemSpec {
    pub project_name: String,
    pub prompt: String,
    pub title: Option<String>,
    pub created: String,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateItemSpec {
    pub id: String,
    pub prompt: Option<String>,
    pub title: Option<String>,
    pub append: Option<String>,
    pub prereq: Vec<String>,
    pub clear_prereq: bool,
    pub commits: Option<String>,
    pub append_report: Option<String>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
    pub tags_clear: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvePendingWorkOutput {
    NotePath(String),
    NoteMarkdown(String),
}

impl ResolvePendingWorkOutput {
    pub fn into_text(self) -> String {
        match self {
            Self::NotePath(text) | Self::NoteMarkdown(text) => text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteItemSpec {
    pub id: String,
    pub completed: String,
    pub report: Option<String>,
    pub commits: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelItemSpec {
    pub id: String,
    pub completed: String,
    pub report: String,
    pub commits: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedItem {
    pub id: String,
    pub project: String,
    pub title: String,
    pub action: ClosedItemAction,
    pub diagnostics: StatusTransitionDiagnostics,
}

impl ClosedItem {
    pub fn to_output_text(&self) -> String {
        format!(
            "{} {} ({} :: {})\n",
            self.action.past_tense(),
            self.id,
            self.project,
            self.title
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedItemAction {
    Done,
    Cancelled,
}

impl ClosedItemAction {
    fn past_tense(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTransitionOutput {
    pub text: String,
    pub diagnostics: StatusTransitionDiagnostics,
    pub review_item: Option<AddedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTransitionDiagnostics {
    pub futuro_renamed_project: Option<String>,
    pub evicted_ids: Vec<String>,
}

impl StatusTransitionDiagnostics {
    pub fn none() -> Self {
        Self {
            futuro_renamed_project: None,
            evicted_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenedItem {
    pub id: String,
    pub project: String,
    pub already_active: bool,
}

impl ReopenedItem {
    pub fn to_output_text(&self) -> String {
        if self.already_active {
            format!("{} already active ({}) — skipped\n", self.id, self.project)
        } else {
            format!("Reopened {} ({})\n", self.id, self.project)
        }
    }
}
