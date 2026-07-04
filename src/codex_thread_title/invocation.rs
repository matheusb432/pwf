//! Pure argv parsing for the shim's launcher/worker protocol:
//! `--title <t> --cwd <dir> --since <unix-secs> -- <codex argv…>`.

use super::{ARG_SEPARATOR, CWD_FLAG, SINCE_FLAG, TITLE_FLAG};

pub(super) struct Invocation {
    pub(super) title: String,
    pub(super) cwd: String,
    pub(super) since: u64,
    pub(super) codex_argv: Vec<String>,
}

impl Invocation {
    pub(super) fn parse(args: &[String]) -> Result<Self, String> {
        let mut title = None;
        let mut cwd = None;
        let mut since = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                ARG_SEPARATOR => {
                    let codex_argv = args[i + 1..].to_vec();
                    if codex_argv.is_empty() {
                        return Err("missing codex argv after --".to_string());
                    }
                    return Ok(Self {
                        title: title.ok_or_else(|| "missing --title".to_string())?,
                        cwd: cwd.ok_or_else(|| "missing --cwd".to_string())?,
                        since: since.ok_or_else(|| "missing --since".to_string())?,
                        codex_argv,
                    });
                }
                TITLE_FLAG => {
                    i += 1;
                    title = args.get(i).cloned();
                }
                CWD_FLAG => {
                    i += 1;
                    cwd = args.get(i).cloned();
                }
                SINCE_FLAG => {
                    i += 1;
                    since = Some(
                        args.get(i)
                            .ok_or_else(|| "missing --since value".to_string())?
                            .parse::<u64>()
                            .map_err(|_| "invalid --since value".to_string())?,
                    );
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Err("missing -- before codex argv".to_string())
    }

    pub(super) fn prompt_prefix(&self) -> Option<&str> {
        self.codex_argv
            .last()
            .and_then(|prompt| prompt.lines().next())
            .filter(|line| !line.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{super::CODEX_BINARY, *};

    #[test]
    fn invocation_parser_keeps_codex_prompt_as_one_arg() {
        let args = vec![
            TITLE_FLAG.to_string(),
            "title".to_string(),
            CWD_FLAG.to_string(),
            "/repo".to_string(),
            SINCE_FLAG.to_string(),
            "42".to_string(),
            ARG_SEPARATOR.to_string(),
            CODEX_BINARY.to_string(),
            ARG_SEPARATOR.to_string(),
            "prompt\n--danger".to_string(),
        ];
        let parsed = Invocation::parse(&args).unwrap();
        assert_eq!(parsed.title, "title");
        assert_eq!(parsed.cwd, "/repo");
        assert_eq!(parsed.since, 42);
        assert_eq!(parsed.codex_argv, vec!["codex", "--", "prompt\n--danger"]);
        assert_eq!(parsed.prompt_prefix(), Some("prompt"));
    }
}
