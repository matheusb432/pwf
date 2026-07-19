use std::str::FromStr;

use pwf_domain::pending_work::PendingWorkItemView;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Item {
    pub id: String,
    pub project: String,
    pub session: String,
    pub prompt: String,
    pub repo: Option<String>,
    pub note: String,
    pub file_path: Option<String>,
    pub line: usize,
    pub format: String,
    pub launchable: bool,
    pub needs_prompt: bool,
    pub issues: Vec<String>,
    // `None` selects the normal section and default list visibility.
    pub section: Option<String>,
    // Raw `prereq` frontmatter.
    pub prereq: Option<String>,
    // Raw `effort` frontmatter; session resolution validates it.
    pub effort: Option<String>,
    // Raw `created` frontmatter; legacy inline items have no value.
    pub created: Option<String>,
}

impl Item {
    pub fn empty() -> Self {
        Item {
            id: String::new(),
            project: String::new(),
            session: String::new(),
            prompt: String::new(),
            repo: None,
            note: String::new(),
            file_path: None,
            line: 0,
            format: String::new(),
            launchable: false,
            needs_prompt: false,
            issues: vec![],
            section: None,
            prereq: None,
            effort: None,
            created: None,
        }
    }
}

impl From<PendingWorkItemView> for Item {
    fn from(item: PendingWorkItemView) -> Self {
        Self {
            id: item.id,
            project: item.project,
            session: item.session,
            prompt: item.prompt,
            repo: item.repo,
            note: item.note,
            file_path: item.item_file,
            line: item.line,
            format: item.format,
            launchable: item.launchable,
            needs_prompt: item.needs_prompt,
            issues: item.issues,
            section: item.section,
            prereq: item.prereq,
            effort: item.effort,
            created: item.created,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Route,
    Add,
    List,
    Verify,
    Done,
    Cancel,
    Reopen,
    Show,
    Remove,
    Update,
    Session,
}

const ROUTE: &str = "route";
const ADD: &str = "add";
const LIST: &str = "list";
const VERIFY: &str = "verify";
const DONE: &str = "done";
const CANCEL: &str = "cancel";
const REOPEN: &str = "reopen";
const SHOW: &str = "show";
const REMOVE: &str = "remove";
const UPDATE: &str = "update";
const SESSION: &str = "session";

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Route => ROUTE,
            Action::Add => ADD,
            Action::List => LIST,
            Action::Verify => VERIFY,
            Action::Done => DONE,
            Action::Cancel => CANCEL,
            Action::Reopen => REOPEN,
            Action::Show => SHOW,
            Action::Remove => REMOVE,
            Action::Update => UPDATE,
            Action::Session => SESSION,
        }
    }
}

#[cfg(test)]
impl Item {
    pub fn default_for_test(id: &str, session: &str) -> Self {
        Item {
            id: id.to_string(),
            project: "pwf".to_string(),
            session: session.to_string(),
            prompt: String::new(),
            repo: None,
            note: String::new(),
            file_path: None,
            line: 0,
            format: "file".to_string(),
            launchable: true,
            needs_prompt: false,
            issues: vec![],
            section: None,
            prereq: None,
            effort: None,
            created: None,
        }
    }
}

impl FromStr for Action {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            ROUTE => Ok(Action::Route),
            ADD => Ok(Action::Add),
            LIST => Ok(Action::List),
            VERIFY => Ok(Action::Verify),
            DONE => Ok(Action::Done),
            CANCEL => Ok(Action::Cancel),
            REOPEN => Ok(Action::Reopen),
            SHOW => Ok(Action::Show),
            REMOVE => Ok(Action::Remove),
            UPDATE => Ok(Action::Update),
            SESSION => Ok(Action::Session),
            _ => Err(()),
        }
    }
}
