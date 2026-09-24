//! Aliases that take arguments, like `mkcd` = `mkdir -p "$1" && cd "$1"`.
//!
//! Commands use bash-style placeholders. aka turns such an alias into a
//! function and translates the placeholders for fish and PowerShell.

/// How one shell spells each placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Posix,
    Fish,
    Powershell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    /// `$1` to `$9`, or `${1}`
    Nth(u8),
    /// `"$@"`: every argument, each kept whole
    AllQuoted,
    /// `$@`
    All,
    /// `$*`: every argument joined into one string
    Joined,
    /// `$#`: how many arguments there are
    Count,
}

/// Finds the placeholder starting at `i`, and how many bytes it spans.
fn placeholder_at(s: &str, i: usize) -> Option<(Placeholder, usize)> {
    let rest = &s[i..];
    if rest.starts_with("\"$@\"") {
        return Some((Placeholder::AllQuoted, 4));
    }
    let after = rest.strip_prefix('$')?;
    let mut chars = after.chars();
    match chars.next()? {
        '@' => Some((Placeholder::All, 2)),
        '*' => Some((Placeholder::Joined, 2)),
        '#' => Some((Placeholder::Count, 2)),
        c @ '1'..='9' => Some((Placeholder::Nth(c as u8 - b'0'), 2)),
        '{' => {
            let digit = chars.next()?;
            (('1'..='9').contains(&digit) && chars.next()? == '}')
                .then(|| (Placeholder::Nth(digit as u8 - b'0'), 4))
        }
        _ => None,
    }
}

/// True when the command uses any argument placeholder.
pub fn uses_placeholders(command: &str) -> bool {
    command
        .char_indices()
        .any(|(i, _)| placeholder_at(command, i).is_some())
}

/// Rewrites the placeholders for `style`. Everything else stays as written.
pub fn translate(command: &str, style: Style) -> String {
    if style == Style::Posix {
        return command.to_string();
    }
    let mut out = String::with_capacity(command.len() + 16);
    let mut i = 0;
    while i < command.len() {
        if let Some((p, len)) = placeholder_at(command, i) {
            out.push_str(&spell(p, style));
            i += len;
        } else {
            let c = command[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn spell(p: Placeholder, style: Style) -> String {
    match (style, p) {
        (Style::Posix, _) => unreachable!("posix keeps placeholders as they are"),
        // fish lists don't split on spaces, so $argv already keeps arguments whole
        (Style::Fish, Placeholder::Nth(n)) => format!("$argv[{n}]"),
        (Style::Fish, Placeholder::AllQuoted | Placeholder::All) => "$argv".into(),
        (Style::Fish, Placeholder::Joined) => "\"$argv\"".into(),
        (Style::Fish, Placeholder::Count) => "(count $argv)".into(),
        // $(...) so it also works inside double quotes
        (Style::Powershell, Placeholder::Nth(n)) => format!("$($args[{}])", n - 1),
        (Style::Powershell, Placeholder::AllQuoted | Placeholder::All) => "@args".into(),
        (Style::Powershell, Placeholder::Joined) => "\"$args\"".into(),
        (Style::Powershell, Placeholder::Count) => "$args.Count".into(),
    }
}

/// The command with placeholders replaced by a plain word, for checks that
/// shouldn't mistake them for shell-specific syntax.
pub fn without_placeholders(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut i = 0;
    while i < command.len() {
        if let Some((_, len)) = placeholder_at(command, i) {
            out.push_str("arg");
            i += len;
        } else {
            let c = command[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_placeholders() {
        assert!(uses_placeholders("mkdir -p \"$1\" && cd \"$1\""));
        assert!(uses_placeholders("echo ${2}"));
        assert!(uses_placeholders("grep -r \"$@\" ."));
        assert!(uses_placeholders("echo $#"));
        assert!(!uses_placeholders("echo $HOME"));
        assert!(!uses_placeholders("echo $$ $0"));
        assert!(!uses_placeholders("git status"));
    }

    #[test]
    fn translates_for_fish() {
        assert_eq!(
            translate("mkdir -p \"$1\" && cd \"$1\"", Style::Fish),
            "mkdir -p \"$argv[1]\" && cd \"$argv[1]\""
        );
        assert_eq!(
            translate("grep -r \"$@\" .", Style::Fish),
            "grep -r $argv ."
        );
        assert_eq!(
            translate("echo $# ${2}", Style::Fish),
            "echo (count $argv) $argv[2]"
        );
    }

    #[test]
    fn translates_for_powershell() {
        assert_eq!(
            translate("mkdir -p \"$1\" && cd \"$1\"", Style::Powershell),
            "mkdir -p \"$($args[0])\" && cd \"$($args[0])\""
        );
        assert_eq!(
            translate("grep -r \"$@\" .", Style::Powershell),
            "grep -r @args ."
        );
        assert_eq!(translate("echo $*", Style::Powershell), "echo \"$args\"");
    }

    #[test]
    fn posix_is_untouched() {
        let cmd = "mkdir -p \"$1\" && cd \"$1\"";
        assert_eq!(translate(cmd, Style::Posix), cmd);
        assert_eq!(
            without_placeholders("cd \"$1\" && ls ${2}"),
            "cd \"arg\" && ls arg"
        );
    }
}
