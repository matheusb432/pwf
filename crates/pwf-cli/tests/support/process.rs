use std::process::Command;

use assert_cmd::prelude::OutputAssertExt as _;
use serde_json::Value;

pub trait CommandTestExt {
    fn color(&mut self) -> &mut Self;
    fn success_stdout(&mut self) -> String;
    fn success_json(&mut self) -> Value;
}

impl CommandTestExt for Command {
    fn color(&mut self) -> &mut Self {
        self.env_remove("NO_COLOR").env("CLICOLOR_FORCE", "1")
    }

    fn success_stdout(&mut self) -> String {
        let assertion = self.assert().success().stderr("");
        String::from_utf8(assertion.get_output().stdout.clone()).unwrap()
    }

    fn success_json(&mut self) -> Value {
        serde_json::from_str(&self.success_stdout()).unwrap()
    }
}
