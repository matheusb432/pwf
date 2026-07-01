use std::str::FromStr;

/// Task model shared across the pending-work submodules.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Item {
    pub id: String,
    pub project: String,
    pub session: String,
    pub prompt: String,
    pub repo: Option<String>,
    pub note: String,
    pub item_file: Option<String>,
    pub line: usize,
    pub format: String,
    // ? Internal cursor fields for index rewriting; not part of the data model.
    pub marker_index: usize,
    pub marker_length: usize,
    pub launchable: bool,
    pub needs_prompt: bool,
    pub issues: Vec<String>,
    // ? Index section governing this item (`Future`/`Human`), or `None` for the
    // ? normal/visible region. Internal-only: drives default list filtering.
    pub section: Option<String>,
    // ? Raw `prereq` frontmatter (e.g. "[[CFG-0014]]").
    pub prereq: Option<String>,
    // ? Raw `effort` frontmatter (e.g. "3"); unvalidated here — EffortTier::parse
    // ? validates it at resolution time (session/verify).
    pub effort: Option<String>,
    // ? Raw `created` frontmatter (e.g. "2026-01-01"); `None` for legacy inline
    // ? items, which have no backing note. Drives `list --order created`.
    pub created: Option<String>,
}

impl Item {
    pub fn empty() -> Self {
        Item {
            id: "".into(),
            project: "".into(),
            session: "".into(),
            prompt: "".into(),
            repo: None,
            note: "".into(),
            item_file: None,
            line: 0,
            format: "".into(),
            marker_index: 0,
            marker_length: 0,
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

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Route,
    Add,
    List,
    Clean,
    Verify,
    Check,
    Cancel,
    Reopen,
    Resolve,
    Show,
    Remove,
    Update,
    Session,
}

const ROUTE: &str = "route";
const ADD: &str = "add";
const LIST: &str = "list";
const CLEAN: &str = "clean";
const VERIFY: &str = "verify";
const CHECK: &str = "check";
const CANCEL: &str = "cancel";
const REOPEN: &str = "reopen";
const RESOLVE: &str = "resolve";
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
            Action::Clean => CLEAN,
            Action::Verify => VERIFY,
            Action::Check => CHECK,
            Action::Cancel => CANCEL,
            Action::Reopen => REOPEN,
            Action::Resolve => RESOLVE,
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
            item_file: None,
            line: 0,
            format: "file".to_string(),
            marker_index: 0,
            marker_length: 0,
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
            CLEAN => Ok(Action::Clean),
            VERIFY => Ok(Action::Verify),
            CHECK => Ok(Action::Check),
            CANCEL => Ok(Action::Cancel),
            REOPEN => Ok(Action::Reopen),
            RESOLVE => Ok(Action::Resolve),
            SHOW => Ok(Action::Show),
            REMOVE => Ok(Action::Remove),
            UPDATE => Ok(Action::Update),
            SESSION => Ok(Action::Session),
            _ => Err(()),
        }
    }
}
