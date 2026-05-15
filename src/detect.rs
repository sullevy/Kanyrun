use crate::cli::CliRequest;
use crate::context::{Context, ContextKind, ContextSource};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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
        return Ok(classify_paths(
            normalize_path_candidates(request.files.iter().map(|path| path.as_path())),
            ContextSource::ExplicitFiles,
        ));
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
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }

    let paths: Vec<PathBuf> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| normalize_existing_path(line))
        .collect();

    if paths.len() != lines.len() {
        return None;
    }

    Some(deduplicate_paths(paths))
}

fn normalize_path_candidates<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<PathBuf> {
    deduplicate_paths(
        paths
            .into_iter()
            .map(|path| normalize_path_lossy(path.to_string_lossy().as_ref()))
            .collect(),
    )
}

fn normalize_existing_path(raw: &str) -> Option<PathBuf> {
    let path = normalize_path_lossy(raw);
    if path.exists() {
        Some(fs::canonicalize(&path).unwrap_or(path))
    } else {
        None
    }
}

fn normalize_path_lossy(raw: &str) -> PathBuf {
    let trimmed = raw
        .trim_end_matches('\r')
        .trim_matches('"')
        .trim_matches('\'');
    let decoded = if let Some(uri) = trimmed.strip_prefix("file://") {
        let without_host = uri.strip_prefix("localhost").unwrap_or(uri);
        decode_percent(without_host)
    } else {
        trimmed.to_string()
    };
    let path = PathBuf::from(decoded);
    if path.exists() {
        fs::canonicalize(&path).unwrap_or(path)
    } else {
        path
    }
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }
    unique
}

fn decode_percent(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
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

        let context = resolve_context(
            &request,
            &FakeSelectionReader {
                clipboard: None,
                primary: None,
            },
        )
        .unwrap();

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

        let context = resolve_context(
            &request,
            &FakeSelectionReader {
                clipboard: None,
                primary: None,
            },
        )
        .unwrap();

        assert_eq!(context.source, ContextSource::ExplicitFiles);
        assert_eq!(context.kind, ContextKind::Directory);
        assert_eq!(context.files, vec![temp.path().to_path_buf()]);
    }

    #[test]
    fn classifies_newline_separated_clipboard_paths_as_file_list_when_requested() {
        let temp = TempDir::new().unwrap();
        let first = temp.path().join("one.txt");
        let second = temp.path().join("two.pdf");
        fs::write(&first, "one").unwrap();
        fs::write(&second, "two").unwrap();

        let reader = FakeSelectionReader {
            clipboard: Some(format!("{}\n{}", first.display(), second.display())),
            primary: None,
        };

        let request = CliRequest {
            from_clipboard: true,
            ..CliRequest::default()
        };

        let context = resolve_context(&request, &reader).unwrap();

        assert_eq!(context.source, ContextSource::Clipboard);
        assert_eq!(context.kind, ContextKind::FileList);
        assert_eq!(context.files, vec![first, second]);
        assert_eq!(
            context.mime_types,
            vec!["text/plain".to_string(), "application/pdf".to_string()]
        );
    }

    #[test]
    fn defaults_to_empty_even_if_primary_has_text() {
        let reader = FakeSelectionReader {
            clipboard: Some("clipboard text".into()),
            primary: Some("search text".into()),
        };

        let context = resolve_context(&CliRequest::default(), &reader).unwrap();

        assert_eq!(context.source, ContextSource::Empty);
        assert_eq!(context.kind, ContextKind::Empty);
        assert!(context.text.is_none());
    }

    #[test]
    fn defaults_to_empty_when_primary_is_empty_even_if_clipboard_has_text() {
        let reader = FakeSelectionReader {
            clipboard: Some("clipboard text".into()),
            primary: Some("   \n".into()),
        };

        let context = resolve_context(&CliRequest::default(), &reader).unwrap();

        assert_eq!(context.source, ContextSource::Empty);
        assert_eq!(context.kind, ContextKind::Empty);
        assert_eq!(context.text.as_deref(), None);
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

        let context = resolve_context(
            &request,
            &FakeSelectionReader {
                clipboard: None,
                primary: None,
            },
        )
        .unwrap();

        assert_eq!(context.kind, ContextKind::File);
        assert_eq!(context.mime_types, vec!["application/pdf".to_string()]);
    }

    #[test]
    fn normalizes_file_uri_from_explicit_files() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("report file.pdf");
        fs::write(&file, "%PDF-1.4").unwrap();
        let uri = format!("file://{}", file.display().to_string().replace(' ', "%20"));

        let request = CliRequest {
            files: vec![PathBuf::from(uri)],
            ..CliRequest::default()
        };

        let context = resolve_context(
            &request,
            &FakeSelectionReader {
                clipboard: None,
                primary: None,
            },
        )
        .unwrap();

        assert_eq!(context.kind, ContextKind::File);
        assert_eq!(context.files, vec![fs::canonicalize(&file).unwrap()]);
        assert_eq!(context.mime_types, vec!["application/pdf".to_string()]);
    }

    #[test]
    fn normalizes_file_uri_from_clipboard_text() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("Project Folder");
        fs::create_dir(&dir).unwrap();
        let uri = format!(
            "file://localhost{}",
            dir.display().to_string().replace(' ', "%20")
        );

        let request = CliRequest {
            from_clipboard: true,
            ..CliRequest::default()
        };
        let reader = FakeSelectionReader {
            clipboard: Some(uri),
            primary: None,
        };

        let context = resolve_context(&request, &reader).unwrap();

        assert_eq!(context.kind, ContextKind::Directory);
        assert_eq!(context.files, vec![fs::canonicalize(&dir).unwrap()]);
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
