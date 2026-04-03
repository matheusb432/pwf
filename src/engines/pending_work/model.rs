use std::str::FromStr;

/// Task model shared across the pending-work submodules.
#[derive(PartialEq, Eq, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(skip)]
    pub marker_index: usize,
    #[serde(skip)]
    pub marker_length: usize,
    pub launchable: bool,
    pub needs_prompt: bool,
    pub issues: Vec<String>,
    // ? Index section governing this item (`Future`/`Human`), or `None` for the
    // ? normal/visible region. Internal-only: drives default list filtering.
    #[serde(skip)]
    pub section: Option<String>,
    // ? Raw `prereq` frontmatter (e.g. "[[CFG-0014]]"). Omitted from JSON when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prereq: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Route,
    New,
    Add,
    List,
    Clean,
    Verify,
    LaunchClaude,
    Check,
    Launch,
    Resolve,
    Remove,
    Update,
}

const ROUTE: &str = "route";
const NEW: &str = "new";
const ADD: &str = "add";
const LIST: &str = "list";
const CLEAN: &str = "clean";
const VERIFY: &str = "verify";
const LAUNCH_CLAUDE: &str = "launch-claude";
const CHECK: &str = "check";
const LAUNCH: &str = "launch";
const RESOLVE: &str = "resolve";
const REMOVE: &str = "remove";
const UPDATE: &str = "update";

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Route => ROUTE,
            Action::New => NEW,
            Action::Add => ADD,
            Action::List => LIST,
            Action::Clean => CLEAN,
            Action::Verify => VERIFY,
            Action::LaunchClaude => LAUNCH_CLAUDE,
            Action::Check => CHECK,
            Action::Launch => LAUNCH,
            Action::Resolve => RESOLVE,
            Action::Remove => REMOVE,
            Action::Update => UPDATE,
        }
    }
}

impl FromStr for Action {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            ROUTE => Ok(Action::Route),
            NEW => Ok(Action::New),
            ADD => Ok(Action::Add),
            LIST => Ok(Action::List),
            CLEAN => Ok(Action::Clean),
            VERIFY => Ok(Action::Verify),
            LAUNCH_CLAUDE => Ok(Action::LaunchClaude),
            CHECK => Ok(Action::Check),
            LAUNCH => Ok(Action::Launch),
            RESOLVE => Ok(Action::Resolve),
            REMOVE => Ok(Action::Remove),
            UPDATE => Ok(Action::Update),
            _ => Err(()),
        }
    }
}
