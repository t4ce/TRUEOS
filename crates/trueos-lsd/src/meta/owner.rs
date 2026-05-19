use super::metadata::Metadata;
use crate::color::{ColoredString, Colors, Elem};
use crate::Flags;

#[derive(Default)]
pub struct Cache;

#[derive(Clone, Debug, Default)]
pub struct Owner {
    user: u32,
    group: u32,
}

impl From<&Metadata> for Owner {
    fn from(_: &Metadata) -> Self {
        Self::default()
    }
}

fn truncate(input: &str, after: Option<usize>, marker: Option<String>) -> String {
    let mut output = input.to_string();

    if let Some(after) = after {
        if output.len() > after {
            output.truncate(after);

            if let Some(marker) = marker {
                output.push_str(&marker);
            }
        }
    }

    output
}

impl Owner {
    // allow unused variables because cache is used in unix, maybe we can cache for windows in the future
    #[allow(unused_variables)]
    pub fn render_user(&self, colors: &Colors, cache: &Cache, flags: &Flags) -> ColoredString {
        let user = self.user.to_string();

        colors.colorize(
            truncate(&user, flags.truncate_owner.after, flags.truncate_owner.marker.clone()),
            &Elem::User,
        )
    }

    // allow unused variables because cache is used in unix, maybe we can cache for windows in the future
    #[allow(unused_variables)]
    pub fn render_group(&self, colors: &Colors, cache: &Cache, flags: &Flags) -> ColoredString {
        let group = self.group.to_string();

        colors.colorize(
            truncate(&group, flags.truncate_owner.after, flags.truncate_owner.marker.clone()),
            &Elem::Group,
        )
    }
}

#[cfg(test)]
mod test_truncate {
    use crate::meta::owner::truncate;

    #[test]
    fn test_none() {
        assert_eq!("a", truncate("a", None, None));
    }

    #[test]
    fn test_unchanged_without_marker() {
        assert_eq!("a", truncate("a", Some(1), None));
    }

    #[test]
    fn test_unchanged_with_marker() {
        assert_eq!("a", truncate("a", Some(1), Some("…".to_string())));
    }

    #[test]
    fn test_truncated_without_marker() {
        assert_eq!("a", truncate("ab", Some(1), None));
    }

    #[test]
    fn test_truncated_with_marker() {
        assert_eq!("a…", truncate("ab", Some(1), Some("…".to_string())));
    }
}
