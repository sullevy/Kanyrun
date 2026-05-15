use crate::cli::CliRequest;
use crate::context::{Context, ContextKind, ContextSource};
use std::path::PathBuf;

pub trait SelectionReader {
    fn read_clipboard(&self) -> Result<Option<String>, String>;
    fn read_primary(&self) -> Result<Option<String>, String>;
}

pub fn resolve_context<R>(request: &CliRequest, reader: &R) -> Result<Context, String>
where
    R: SelectionReader,
{
    if let Some(text) = request.text.as_deref() {
        if let Some(context) = classify_text(text, ContextSource::ExplicitText) {
            return Ok(context);
        }
    }

    if !request.files.is_empty() {
        return Ok(classify_paths(request.files.clone(), ContextSource::ExplicitFiles));
    }

    for source in selection_order(request) {
        let content = match source {
            ContextSource::Clipboard => reader.read_clipboard()?,
            ContextSource::PrimarySelection => reader.read_primary()?,
            _ => None,
        };

        if let Some(text) = content {
            if let Some(context) = classify_text(&text, source) {
                return Ok(context);
            }
        }
    }

    Ok(Context::empty(ContextSource::Empty))
}

fn selection_order(request: &CliRequest) -> Vec<ContextSource> {
    let mut order = Vec::new();

    if request.from_primary {
        order.push(ContextSource::PrimarySelection);
    }

    if request.from_clipboard {
        order.push(ContextSource::Clipboard);
    }

    if order.is_empty() {
        order.push(ContextSource::PrimarySelection);
        order.push(ContextSource::Clipboard);
    }

    order
}

fn classify_text(raw: &str, source: ContextSource) -> Option<Context> {
    let normalized = raw.trim();

    if normalized.is_empty() {
        return None;
    }

    if let Some(paths) = parse_existing_paths(normalized) {
        return Some(classify_paths(paths, source));
    }

    let kind = if looks_like_url(normalized) {
        ContextKind::Url
    } else {
        ContextKind::Text
    };

    Some(Context {
        source,
        kind,
        raw: normalized.to_string(),
        text: Some(normalized.to_string()),
        files: Vec::new(),
        mime_types: Vec::new(),
    })
}

fn parse_existing_paths(raw: &str) -> Option<Vec<PathBuf>> {
    let paths: Vec<PathBuf> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect();

    if paths.is_empty() {
        return None;
    }

    if paths.iter().all(|path| path.exists()) {
        Some(paths)
    } else {
        None
    }
}

fn classify_paths(paths: Vec<PathBuf>, source: ContextSource) -> Context {
    if paths.is_empty() {
        return Context::empty(source);
    }

    let kind = if paths.len() > 1 {
        ContextKind::FileList
    } else {
        classify_single_path(&paths[0])
    };

    Context {
        source,
        kind,
        raw: paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        text: None,
        mime_types: collect_mime_types(&paths),
        files: paths,
    }
}

fn classify_single_path(path: &PathBuf) -> ContextKind {
    if path.is_dir() {
        ContextKind::Directory
    } else if path.is_file() {
        ContextKind::File
    } else {
        ContextKind::Path
    }
}

fn collect_mime_types(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| path.is_file())
        .filter_map(|path| mime_guess::from_path(path).first_raw().map(str::to_string))
        .collect()
}

fn looks_like_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::{SelectionReader, resolve_context};
    use crate::cli::CliRequest;
    use crate::context::{ContextKind, ContextSource};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct FakeSelectionReader {
        clipboard: Option<String>,
        primary: Option<String>,
    }

    impl SelectionReader for FakeSelectionReader {
        fn read_clipboard(&self) -> Result<Option<String>, String> {
            Ok(self.clipboard.clone())
        }

        fn read_primary(&self) -> Result<Option<String>, String> {
            Ok(self.primary.clone())
        }
    }

    #[test]
    fn classifies_explicit_url_text_as_url() {
        let request = CliRequest {
            text: Some("https://example.com".into()),
            ..CliRequest::default()
        };

        let context = resolve_context(&request, &FakeSelectionReader { clipboard: None, primary: None }).unwrap();

        assert_eq!(context.source, ContextSource::ExplicitText);
        assert_eq!(context.kind, ContextKind::Url);
        assert_eq!(context.text.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn classifies_single_directory_from_explicit_files() {
        let temp = TempDir::new().unwrap();
        let request = CliRequest {
            files: vec![temp.path().to_path_buf()],
            ..CliRequest::default()
        };

        let context = resolve_context(&request, &FakeSelectionReader { clipboard: None, primary: None }).unwrap();

        assert_eq!(context.source, ContextSource::ExplicitFiles);
        assert_eq!(context.kind, ContextKind::Directory);
        assert_eq!(context.files, vec![temp.path().to_path_buf()]);
    }

    #[test]
    fn classifies_newline_separated_existing_paths_as_file_list() {
        let temp = TempDir::new().unwrap();
        let first = temp.path().join("one.txt");
        let second = temp.path().join("two.pdf");
        fs::write(&first, "one").unwrap();
        fs::write(&second, "two").unwrap();

        let reader = FakeSelectionReader {
            clipboard: Some(format!("{}\n{}", first.display(), second.display())),
            primary: None,
        };

        let context = resolve_context(&CliRequest::default(), &reader).unwrap();

        assert_eq!(context.source, ContextSource::Clipboard);
        assert_eq!(context.kind, ContextKind::FileList);
        assert_eq!(context.files, vec![first, second]);
        assert_eq!(context.mime_types, vec!["text/plain".to_string(), "application/pdf".to_string()]);
    }

    #[test]
    fn prefers_primary_selection_by_default() {
        let reader = FakeSelectionReader {
            clipboard: Some("clipboard text".into()),
            primary: Some("search text".into()),
        };

        let context = resolve_context(&CliRequest::default(), &reader).unwrap();

        assert_eq!(context.source, ContextSource::PrimarySelection);
        assert_eq!(context.kind, ContextKind::Text);
        assert_eq!(context.text.as_deref(), Some("search text"));
    }

    #[test]
    fn falls_back_to_clipboard_when_primary_is_empty() {
        let reader = FakeSelectionReader {
            clipboard: Some("clipboard text".into()),
            primary: Some("   \n".into()),
        };

        let context = resolve_context(&CliRequest::default(), &reader).unwrap();

        assert_eq!(context.source, ContextSource::Clipboard);
        assert_eq!(context.kind, ContextKind::Text);
        assert_eq!(context.text.as_deref(), Some("clipboard text"));
    }

    #[test]
    fn detects_pdf_mime_for_single_file() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("doc.pdf");
        fs::write(&file, "%PDF-1.4").unwrap();

        let request = CliRequest {
            files: vec![PathBuf::from(&file)],
            ..CliRequest::default()
        };

        let context = resolve_context(&request, &FakeSelectionReader { clipboard: None, primary: None }).unwrap();

        assert_eq!(context.kind, ContextKind::File);
        assert_eq!(context.mime_types, vec!["application/pdf".to_string()]);
    }

    #[test]
    fn honors_from_primary_flag_over_clipboard() {
        let request = CliRequest {
            from_primary: true,
            ..CliRequest::default()
        };
        let reader = FakeSelectionReader {
            clipboard: Some("clipboard text".into()),
            primary: Some("primary text".into()),
        };

        let context = resolve_context(&request, &reader).unwrap();

        assert_eq!(context.source, ContextSource::PrimarySelection);
        assert_eq!(context.text.as_deref(), Some("primary text"));
    }

    #[test]
    fn honors_from_clipboard_flag_over_primary() {
        let request = CliRequest {
            from_clipboard: true,
            ..CliRequest::default()
        };
        let reader = FakeSelectionReader {
            clipboard: Some("clipboard text".into()),
            primary: Some("primary text".into()),
        };

        let context = resolve_context(&request, &reader).unwrap();

        assert_eq!(context.source, ContextSource::Clipboard);
        assert_eq!(context.text.as_deref(), Some("clipboard text"));
    }
}
