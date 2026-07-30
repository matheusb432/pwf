/// Contains a parsed title and four ordered bullet sections.
///
/// `title` is case-preserved and remains separate from the authored Goals bullets.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedPrompt {
    pub title: String,
    pub goals: Vec<String>,
    pub context: Vec<String>,
    pub constraints: Vec<String>,
    pub done_when: Vec<String>,
}
