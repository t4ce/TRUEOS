#![no_std]
//! UTF-8 slash paths are data, independent of host filesystem and Unix APIs.
//!
//! The explicit grammar also keeps a selector such as `trueosfs:disc7` a normal
//! first component. Backslashes are ordinary characters, not separators.

pub use typed_path::{
    Utf8UnixComponent as Component, Utf8UnixComponents as Components, Utf8UnixPath as Path,
    Utf8UnixPathBuf as PathBuf,
};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{format, string::ToString, vec::Vec};
    use typed_path::Utf8Component;

    #[test]
    fn accepted_utf8_paths_match_existing_slash_semantics() {
        let cases = [
            "",
            ".",
            "..",
            "/",
            "//",
            "///a//b/",
            "./a/./b/../c",
            "a/..",
            "a/../",
            "/../a",
            "a/.hidden",
            ".hidden",
            "a/file.tar.lz4",
            "a/file.",
            "trueosfs:disc7/apps/termdir",
            "trueosfs:disc7/",
            "é/目录/🦀.txt",
            "a\\b/c:d",
            " space /x\ny",
        ];
        for text in cases {
            let path = Path::new(text);
            let old = std::path::Path::new(text);
            assert_eq!(path.is_absolute(), old.is_absolute(), "{text:?}");
            assert_eq!(path.has_root(), old.has_root(), "{text:?}");
            assert_eq!(
                path.parent().map(Path::as_str),
                old.parent().map(|p| p.to_str().unwrap()),
                "{text:?}"
            );
            assert_eq!(
                path.file_name(),
                old.file_name().map(|p| p.to_str().unwrap()),
                "{text:?}"
            );
            assert_eq!(
                path.file_stem(),
                old.file_stem().map(|p| p.to_str().unwrap()),
                "{text:?}"
            );
            assert_eq!(
                path.extension(),
                old.extension().map(|p| p.to_str().unwrap()),
                "{text:?}"
            );
            let components: Vec<_> = path.components().map(|c| c.as_str().to_string()).collect();
            let previous: Vec<_> = old
                .components()
                .map(|c| c.as_os_str().to_str().unwrap().to_string())
                .collect();
            assert_eq!(components, previous, "{text:?}");
            for tail in [
                "",
                "leaf.txt",
                "../peer",
                "/replacement",
                "trueosfs:disc2/root",
            ] {
                // Joining an empty name may spell the same path without a trailing
                // slash (`.` instead of `./`). Compare component semantics.
                assert_eq!(
                    std::path::Path::new(path.join(tail).as_str()),
                    old.join(tail),
                    "{text:?} + {tail:?}"
                );
            }
        }
    }

    #[test]
    fn component_boundaries_and_selector_are_preserved() {
        let path = Path::new("trueosfs:disc7/dir/../file.txt");
        assert_eq!(
            path.strip_prefix("trueosfs:disc7").unwrap().as_str(),
            "dir/../file.txt"
        );
        assert!(path.strip_prefix("trueosfs:disc").is_err());
        assert!(matches!(
            path.components().next(),
            Some(Component::Normal("trueosfs:disc7"))
        ));
        assert_eq!(format!("{path}"), path.as_str());
    }
}
