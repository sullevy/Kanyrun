use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    ExplicitText,
    ExplicitFiles,
    Clipboard,
    PrimarySelection,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    Url,
    Text,
    Path,
    File,
    Directory,
    FileList,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub source: ContextSource,
    pub kind: ContextKind,
    pub raw: String,
    pub text: Option<String>,
    pub files: Vec<PathBuf>,
    pub mime_types: Vec<String>,
}

impl Context {
    pub fn empty(source: ContextSource) -> Self {
        Self {
            source,
            kind: ContextKind::Empty,
            raw: String::new(),
            text: None,
            files: Vec::new(),
            mime_types: Vec::new(),
        }
    }
}
