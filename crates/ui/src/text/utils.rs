use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{ImageSource, Resource, SharedUri};

const NUMBERED_PREFIXES_1: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NUMBERED_PREFIXES_2: &str = "abcdefghijklmnopqrstuvwxyz";

const BULLETS: [&str; 5] = ["•", "◦", "▪", "‣", "⁃"];

/// Returns the prefix for a list item.
pub(super) fn list_item_prefix(ix: usize, ordered: bool, depth: usize) -> String {
    if ordered {
        if depth == 0 {
            return format!("{}. ", ix + 1);
        }

        if depth == 1 {
            return format!(
                "{}. ",
                NUMBERED_PREFIXES_1
                    .chars()
                    .nth(ix % NUMBERED_PREFIXES_1.len())
                    .unwrap()
            );
        } else {
            return format!(
                "{}. ",
                NUMBERED_PREFIXES_2
                    .chars()
                    .nth(ix % NUMBERED_PREFIXES_2.len())
                    .unwrap()
            );
        }
    } else {
        let depth = depth.min(BULLETS.len() - 1);
        let bullet = BULLETS[depth];
        return format!("{} ", bullet);
    }
}

/// Converts a document image URL into an [`ImageSource`].
///
/// With no `base`, a document is granted no filesystem access at all: every
/// value stays URI-backed, including `file://` and scheme-less strings, so
/// untrusted Markdown cannot name a local file and have it read.
///
/// With a `base` the caller has said the document is a local file it trusts.
/// Anything that is not a remote or inline URL is then a path: an absolute one
/// is read as it is, a relative one against `base`, and a `file://` URL has its
/// scheme removed. See [`super::TextViewStyle::image_base`].
pub(super) fn image_source(url: &SharedUri, base: Option<&Path>) -> ImageSource {
    let Some(base) = base else {
        return url.clone().into();
    };

    let text = url.as_ref();
    // A URL the machine can fetch or decode on its own stays one. Everything
    // else is a file name, and no amount of fetching will find it.
    const REMOTE: [&str; 3] = ["http://", "https://", "data:"];
    if REMOTE.iter().any(|scheme| text.starts_with(scheme)) {
        return url.clone().into();
    }

    let path = Path::new(text.strip_prefix("file://").unwrap_or(text));
    let path: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    ImageSource::Resource(Resource::Path(Arc::from(path)))
}

#[cfg(test)]
mod tests {
    use gpui::{ImageSource, Resource};

    use crate::text::utils::{image_source, list_item_prefix};

    #[test]
    fn test_image_source_without_a_base_reaches_nothing_on_disk() {
        fn source(url: &str) -> Resource {
            match image_source(&url.to_string().into(), None) {
                ImageSource::Resource(resource) => resource,
                _ => panic!("expected a resource for {url:?}"),
            }
        }
        fn assert_uri(url: &str) {
            match source(url) {
                Resource::Uri(uri) => assert_eq!(uri.as_ref(), url),
                other => panic!("expected Uri for {url:?}, got {other:?}"),
            }
        }
        assert_uri("https://example.com/logo.png");
        assert_uri("http://example.com/logo.png");
        assert_uri("data:image/png;base64,iVBORw0KGgo=");

        // Every one of these names a file, and without a base none of them is
        // allowed to reach it.
        assert_uri("website/public/logo.svg");
        assert_uri("./images/a.png");
        assert_uri("../images/a.png");
        assert_uri("/absolute/path/logo.svg");
        assert_uri("file:///absolute/path/logo.svg");
        assert_uri(r"C:\images\logo.png");
        assert_uri("docs/a:b.png");
    }

    #[test]
    fn test_image_source_with_a_base_reads_paths_from_disk() {
        use std::path::{Path, PathBuf};

        let base = Path::new("/docs/adr");
        fn source(url: &str, base: &std::path::Path) -> Resource {
            match image_source(&url.to_string().into(), Some(base)) {
                ImageSource::Resource(resource) => resource,
                _ => panic!("expected a resource for {url:?}"),
            }
        }
        fn assert_path(url: &str, base: &std::path::Path, expected: &str) {
            match source(url, base) {
                Resource::Path(path) => {
                    assert_eq!(path.as_ref(), PathBuf::from(expected).as_path())
                }
                other => panic!("expected Path for {url:?}, got {other:?}"),
            }
        }
        fn assert_uri(url: &str, base: &std::path::Path) {
            match source(url, base) {
                Resource::Uri(uri) => assert_eq!(uri.as_ref(), url),
                other => panic!("expected Uri for {url:?}, got {other:?}"),
            }
        }

        // A relative path is resolved against the document's own directory.
        assert_path("diagrams/flow.svg", base, "/docs/adr/diagrams/flow.svg");
        assert_path("./a.png", base, "/docs/adr/./a.png");
        assert_path("../a.png", base, "/docs/adr/../a.png");

        // An absolute path, with or without a scheme, is taken as it is.
        assert_path("/absolute/logo.svg", base, "/absolute/logo.svg");
        assert_path("file:///absolute/logo.svg", base, "/absolute/logo.svg");

        // What the machine can fetch or decode on its own is still fetched.
        assert_uri("https://example.com/logo.png", base);
        assert_uri("http://example.com/logo.png", base);
        assert_uri("data:image/png;base64,iVBORw0KGgo=", base);
    }

    #[test]
    fn test_list_item_prefix() {
        assert_eq!(list_item_prefix(0, true, 0), "1. ");
        assert_eq!(list_item_prefix(1, true, 0), "2. ");
        assert_eq!(list_item_prefix(2, true, 0), "3. ");
        assert_eq!(list_item_prefix(10, true, 0), "11. ");
        assert_eq!(list_item_prefix(0, true, 1), "A. ");
        assert_eq!(list_item_prefix(1, true, 1), "B. ");
        assert_eq!(list_item_prefix(2, true, 1), "C. ");
        assert_eq!(list_item_prefix(0, true, 2), "a. ");
        assert_eq!(list_item_prefix(1, true, 2), "b. ");
        assert_eq!(list_item_prefix(6, true, 2), "g. ");
        assert_eq!(list_item_prefix(0, true, 1), "A. ");
        assert_eq!(list_item_prefix(0, true, 2), "a. ");
        assert_eq!(list_item_prefix(0, false, 0), "• ");
        assert_eq!(list_item_prefix(0, false, 1), "◦ ");
        assert_eq!(list_item_prefix(0, false, 2), "▪ ");
        assert_eq!(list_item_prefix(0, false, 3), "‣ ");
        assert_eq!(list_item_prefix(0, false, 4), "⁃ ");
    }
}
