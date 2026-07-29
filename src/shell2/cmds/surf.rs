use embassy_executor::Spawner;

use super::super::shell2_cmd::ParseOutcome;
use super::super::shell2_surf::{self, SurfPromptPrefix, SurfSubmit};
use super::super::{ShellBackend2, print_shell_line};
use super::tlb_helper::print_table;

const SURF_MENU_HEADERS: [&str; 2] = ["Subcommand", "Input"];
const SURF_MENU_ROWS: [[&str; 2]; 4] = [
    ["https", "HTTPS host or URL"],
    ["http", "HTTP host or URL"],
    ["file", "TRUEOSFS path or file:// reference"],
    ["html", "Inline HTML markup"],
];

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &SURF_MENU_HEADERS, &SURF_MENU_ROWS);
}

fn parse_invocation(rest: &str) -> Option<(SurfPromptPrefix, &str)> {
    let rest = rest.trim_start();
    let subcommand_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let subcommand = rest.get(..subcommand_end)?;
    let input = rest.get(subcommand_end..)?.trim_start();
    let prefix = if subcommand.eq_ignore_ascii_case("https") {
        SurfPromptPrefix::Https
    } else if subcommand.eq_ignore_ascii_case("http") {
        SurfPromptPrefix::Http
    } else if subcommand.eq_ignore_ascii_case("file") {
        SurfPromptPrefix::File
    } else if subcommand.eq_ignore_ascii_case("html") {
        SurfPromptPrefix::Html
    } else {
        return None;
    };
    Some((prefix, input))
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if trimmed.is_empty()
        || matches!(trimmed, "help" | "-h" | "--help")
        || trimmed.eq_ignore_ascii_case("help")
    {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    let Some((prefix, input)) = parse_invocation(rest) else {
        print_shell_line(io, "surf: expected https, http, file, or html");
        print_usage(io);
        return ParseOutcome::Handled;
    };
    let Some(submit) = shell2_surf::try_parse_with_prefix(input, prefix) else {
        print_shell_line(io, "surf: subcommand input is required");
        print_usage(io);
        return ParseOutcome::Handled;
    };

    match submit {
        SurfSubmit::Html(html) => shell2_surf::load_inline_html(spawner, io, html),
        SurfSubmit::File(file_ref) => {
            shell2_surf::load_file_reference(spawner, io, file_ref.as_str())
        }
        SurfSubmit::Url(url) => shell2_surf::prepare_call_with_url(spawner, io, url.as_str()),
    }

    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::{SurfPromptPrefix, parse_invocation};

    #[test]
    fn parses_all_four_subcommands_without_losing_payload_spacing() {
        let (prefix, input) = parse_invocation("https example.de").unwrap();
        assert!(matches!(prefix, SurfPromptPrefix::Https));
        assert_eq!(input, "example.de");

        let (prefix, input) = parse_invocation("http  example.de/a").unwrap();
        assert!(matches!(prefix, SurfPromptPrefix::Http));
        assert_eq!(input, "example.de/a");

        let (prefix, input) = parse_invocation("file apps/index.html").unwrap();
        assert!(matches!(prefix, SurfPromptPrefix::File));
        assert_eq!(input, "apps/index.html");

        let (prefix, input) = parse_invocation("html <h1>Hello world</h1>").unwrap();
        assert!(matches!(prefix, SurfPromptPrefix::Html));
        assert_eq!(input, "<h1>Hello world</h1>");
    }
}
