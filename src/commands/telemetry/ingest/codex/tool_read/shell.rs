use std::env;
use std::path::{Component, Path, PathBuf};

const READ_COMMANDS: &[&str] = &[
    "cat",
    "sed",
    "head",
    "tail",
    "less",
    "more",
    "bat",
    "get-content",
];

pub(super) fn skill_names(commands: &[String]) -> Vec<String> {
    skill_names_with_home(commands, actual_home().as_deref())
}

pub(super) fn skill_names_with_home(commands: &[String], home: Option<&Path>) -> Vec<String> {
    let Some(home) = home else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for command in commands {
        let mut home_expansion_is_trusted = true;
        for segment in shell_segments(command) {
            if mutates_home(&segment.words) {
                home_expansion_is_trusted = false;
            }
            if segment.conditional {
                continue;
            }
            for operand in path_operands(&segment.words) {
                if let Some(name) = trusted_skill_name(operand, home, home_expansion_is_trusted)
                    && !names.iter().any(|existing| existing == &name)
                {
                    names.push(name);
                }
            }
        }
    }
    names
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expansion {
    None,
    Variable,
    Shell,
}

#[derive(Clone, Debug, Default)]
struct ShellWord {
    text: String,
    parts: Vec<(Expansion, String)>,
    fragment_boundary: bool,
}

impl ShellWord {
    fn push(&mut self, ch: char, expansion: Expansion) {
        self.text.push(ch);
        if !self.fragment_boundary
            && let Some((mode, text)) = self.parts.last_mut()
            && *mode == expansion
        {
            text.push(ch);
        } else {
            self.parts.push((expansion, ch.to_string()));
        }
        self.fragment_boundary = false;
    }

    fn begin_fragment(&mut self) {
        self.fragment_boundary = true;
    }

    fn expandable_prefix(&self, prefix: &str, tilde: bool) -> bool {
        self.text.starts_with(prefix)
            && self.parts.first().is_some_and(|(mode, part)| {
                if tilde {
                    part.starts_with(prefix) && *mode == Expansion::Shell
                } else {
                    part.starts_with(prefix.trim_end_matches(['/', '\\']))
                        && matches!(mode, Expansion::Shell | Expansion::Variable)
                }
            })
    }
}

struct ShellSegment {
    words: Vec<ShellWord>,
    conditional: bool,
}

fn shell_segments(command: &str) -> Vec<ShellSegment> {
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut word = ShellWord::default();
    let mut chars = command.chars().peekable();
    let mut quote = None;
    let mut word_started = false;
    let mut invalid_segment = false;
    let mut conditional = false;
    while let Some(ch) = chars.next() {
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                    word.begin_fragment();
                } else {
                    word.push(ch, Expansion::None);
                }
            }
            Some('"') => match ch {
                '"' => {
                    quote = None;
                    word.begin_fragment();
                }
                '\\' => match chars.next() {
                    Some(escaped @ ('$' | '"' | '\\')) => word.push(escaped, Expansion::None),
                    Some('\n') => {}
                    Some('`') | None => invalid_segment = true,
                    Some(other) => {
                        word.push('\\', Expansion::Variable);
                        word.push(other, Expansion::Variable);
                    }
                },
                '`' => invalid_segment = true,
                '$' if chars.peek() == Some(&'(') => invalid_segment = true,
                _ => word.push(ch, Expansion::Variable),
            },
            _ => match ch {
                '\'' | '"' => {
                    word.begin_fragment();
                    quote = Some(ch);
                    word_started = true;
                }
                '\\' => match chars.next() {
                    Some('\n') => {}
                    Some(escaped) if powershell_path_backslash(&words, &word, escaped) => {
                        word.push('\\', Expansion::Shell);
                        word.push(escaped, Expansion::Shell);
                        word_started = true;
                    }
                    Some(escaped) => {
                        word.push(escaped, Expansion::None);
                        word_started = true;
                    }
                    None => invalid_segment = true,
                },
                '`' => invalid_segment = true,
                '$' if chars.peek() == Some(&'(') => invalid_segment = true,
                '<' if chars.peek() == Some(&'<') => return Vec::new(),
                '<' | '>' => invalid_segment = true,
                '#' if !word_started => {
                    let had_segment = !words.is_empty();
                    for comment in chars.by_ref() {
                        if comment == '\n' {
                            break;
                        }
                    }
                    finish_segment(
                        &mut segments,
                        &mut words,
                        &mut word,
                        &mut word_started,
                        &mut invalid_segment,
                        conditional,
                    );
                    if had_segment {
                        conditional = false;
                    }
                }
                ';' | '\n' => {
                    let had_segment = word_started || !words.is_empty();
                    finish_segment(
                        &mut segments,
                        &mut words,
                        &mut word,
                        &mut word_started,
                        &mut invalid_segment,
                        conditional,
                    );
                    if had_segment {
                        conditional = false;
                    }
                }
                '|' | '&' => {
                    let doubled = chars.peek() == Some(&ch);
                    finish_segment(
                        &mut segments,
                        &mut words,
                        &mut word,
                        &mut word_started,
                        &mut invalid_segment,
                        conditional,
                    );
                    if doubled {
                        chars.next();
                        conditional = true;
                    } else if ch == '&' {
                        conditional = false;
                    }
                }
                whitespace if whitespace.is_whitespace() => {
                    finish_word(&mut words, &mut word, &mut word_started)
                }
                _ => {
                    word.push(ch, Expansion::Shell);
                    word_started = true;
                }
            },
        }
    }
    if quote.is_some() {
        invalid_segment = true;
    }
    finish_segment(
        &mut segments,
        &mut words,
        &mut word,
        &mut word_started,
        &mut invalid_segment,
        conditional,
    );
    segments
}

fn powershell_path_backslash(words: &[ShellWord], word: &ShellWord, escaped: char) -> bool {
    !escaped.is_whitespace()
        && words
            .first()
            .is_some_and(|command| command.text.eq_ignore_ascii_case("get-content"))
        && ["$HOME", "${HOME}", "~"]
            .iter()
            .any(|prefix| word.text.starts_with(prefix))
}

fn finish_word(words: &mut Vec<ShellWord>, word: &mut ShellWord, started: &mut bool) {
    if *started {
        words.push(std::mem::take(word));
        *started = false;
    }
}

fn finish_segment(
    segments: &mut Vec<ShellSegment>,
    words: &mut Vec<ShellWord>,
    word: &mut ShellWord,
    started: &mut bool,
    invalid: &mut bool,
    conditional: bool,
) {
    finish_word(words, word, started);
    if !*invalid && !words.is_empty() {
        segments.push(ShellSegment {
            words: std::mem::take(words),
            conditional,
        });
    } else {
        words.clear();
    }
    *invalid = false;
}

#[derive(Clone, Copy)]
enum OptionKind {
    Flag,
    Value,
    OptionalValue,
    Program,
    Path,
}

fn path_operands(segment: &[ShellWord]) -> Vec<&ShellWord> {
    let Some(command) = segment.first() else {
        return Vec::new();
    };
    let verb = command.text.to_ascii_lowercase();
    if !READ_COMMANDS.contains(&verb.as_str()) {
        return Vec::new();
    }
    let mut paths = Vec::new();
    let mut options = true;
    let mut sed_program = false;
    let mut index = 1usize;
    while index < segment.len() {
        let token = &segment[index].text;
        let normalized = if verb == "get-content" {
            token.to_ascii_lowercase()
        } else {
            token.clone()
        };
        if options && normalized == "--" {
            options = false;
            index += 1;
            continue;
        }
        if options && token.starts_with('-') && token != "-" {
            let (option, inline) = normalized
                .split_once('=')
                .map_or((normalized.as_str(), None), |(key, value)| {
                    (key, Some(value))
                });
            if is_help_or_version(&verb, option) {
                return Vec::new();
            }
            let Some(kind) = option_kind(&verb, option) else {
                return Vec::new();
            };
            match (kind, inline) {
                (OptionKind::Flag, None) => index += 1,
                (OptionKind::Flag | OptionKind::Path, Some(_)) => return Vec::new(),
                (OptionKind::OptionalValue, Some("name" | "descriptor"))
                | (OptionKind::OptionalValue, None) => index += 1,
                (OptionKind::OptionalValue, Some(_)) => return Vec::new(),
                (OptionKind::Value | OptionKind::Program, Some(_)) => {
                    sed_program |= matches!(kind, OptionKind::Program);
                    index += 1;
                }
                (OptionKind::Value | OptionKind::Program | OptionKind::Path, None) => {
                    let Some(value) = segment.get(index + 1) else {
                        return Vec::new();
                    };
                    if matches!(kind, OptionKind::Path) {
                        paths.push(value);
                    }
                    sed_program |= matches!(kind, OptionKind::Program);
                    index += 2;
                }
            }
            continue;
        }
        if verb == "sed" && !sed_program {
            sed_program = true;
        } else {
            paths.push(&segment[index]);
        }
        index += 1;
    }
    paths
}

fn option_kind(verb: &str, option: &str) -> Option<OptionKind> {
    let (flags, values, optional_values, programs, paths) = match verb {
        "cat" => (
            "-A -b -e -E -n -s -t -T -u -v --show-all --number-nonblank --show-ends --number --squeeze-blank --show-tabs --show-nonprinting",
            "",
            "",
            "",
            "",
        ),
        "sed" => (
            "-n -E -r -s -u -z --quiet --silent --regexp-extended --separate --unbuffered --null-data",
            "-l --line-length",
            "",
            "-e -f --expression --file",
            "",
        ),
        "head" => (
            "-q -v -z --quiet --verbose --zero-terminated",
            "-c -n --bytes --lines",
            "",
            "",
            "",
        ),
        "tail" => (
            "-f -F -q -v -z --retry --quiet --silent --verbose --zero-terminated",
            "-c -n -s --bytes --lines --max-unchanged-stats --pid --sleep-interval",
            "--follow",
            "",
            "",
        ),
        "less" => (
            "-E -F -K -L -N -Q -R -S -X -m -M",
            "-b -h -j -k -o -O -P -x -y -z --buffers --max-back-scroll --jump-target --lesskey-file --log-file --LOG-file --prompt --tabs --shift --window",
            "",
            "",
            "",
        ),
        "more" => ("-d -l -f -p -c -s -u", "-n", "", "", ""),
        "bat" => (
            "-A -n -p -u --show-all --number --plain --unbuffered --no-custom-assets",
            "--theme --language --tabs --wrap --terminal-width --file-name --highlight-line --line-range --map-syntax --ignored-suffix --diff-context --pager --color --decorations --italic-text --nonprintable-notation --style",
            "",
            "",
            "",
        ),
        "get-content" => (
            "-raw -wait -force -asbytestream",
            "-readcount -totalcount -tail -filter -include -exclude -encoding -delimiter -stream -credential",
            "",
            "",
            "-path -literalpath",
        ),
        _ => return None,
    };
    [
        (flags, OptionKind::Flag),
        (values, OptionKind::Value),
        (optional_values, OptionKind::OptionalValue),
        (programs, OptionKind::Program),
        (paths, OptionKind::Path),
    ]
    .into_iter()
    .find_map(|(set, kind)| {
        set.split_ascii_whitespace()
            .any(|item| item == option)
            .then_some(kind)
    })
}

fn is_help_or_version(verb: &str, option: &str) -> bool {
    let set = match verb {
        "less" | "more" | "bat" => "--help --version -h -? -V",
        "get-content" => "--help -?",
        _ => "--help --version",
    };
    set.split_ascii_whitespace().any(|item| item == option)
}

fn actual_home() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .and_then(|path| normalize_path(&path))
}

fn trusted_skill_name(
    raw: &ShellWord,
    home: &Path,
    home_expansion_is_trusted: bool,
) -> Option<String> {
    let normalized = normalize_path(&expand_home(raw, home, home_expansion_is_trusted)?)?;
    let ordinary_roots = [
        home.join(".codex/skills"),
        home.join(".agents/skills"),
        home.join(".claude/skills"),
        home.join(".loom-registry/skills"),
        home.join(".vibeguard/installed/skills"),
    ];
    for root in ordinary_roots {
        if let Ok(relative) = normalized.strip_prefix(root) {
            return skill_name_from_relative(relative);
        }
    }
    let relative = normalized
        .strip_prefix(home.join(".codex/plugins/cache"))
        .ok()?;
    let components = normal_components(relative)?;
    let (skills, _) = components
        .iter()
        .enumerate()
        .rev()
        .find(|(index, component)| **component == "skills" && *index > 0)?;
    let suffix = &components[skills + 1..];
    (suffix.len() == 2 && suffix[1] == "SKILL.md" && valid_skill_path_component(suffix[0]))
        .then(|| suffix[0].to_string())
}

fn expand_home(raw: &ShellWord, home: &Path, home_expansion_is_trusted: bool) -> Option<PathBuf> {
    for (prefix, tilde) in [
        ("~/", true),
        ("~\\", true),
        ("$HOME/", false),
        ("$HOME\\", false),
        ("${HOME}/", false),
        ("${HOME}\\", false),
    ] {
        if home_expansion_is_trusted && raw.expandable_prefix(prefix, tilde) {
            return Some(join_home(
                home,
                &raw.text[prefix.len()..],
                prefix.ends_with('\\'),
            ));
        }
    }
    let path = PathBuf::from(&raw.text);
    path.is_absolute().then_some(path)
}

fn join_home(home: &Path, suffix: &str, windows_separator: bool) -> PathBuf {
    if windows_separator {
        suffix
            .split('\\')
            .fold(home.to_path_buf(), |path, component| path.join(component))
    } else {
        home.join(suffix)
    }
}

fn mutates_home(words: &[ShellWord]) -> bool {
    let texts = words
        .iter()
        .map(|word| word.text.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let assignment = |word: &str| {
        ["home=", "userprofile=", "$env:home=", "$env:userprofile="]
            .iter()
            .any(|prefix| word.starts_with(prefix))
    };
    if texts.first().is_some_and(|word| assignment(word)) {
        return true;
    }
    let Some(command) = texts.first().map(String::as_str) else {
        return false;
    };
    matches!(
        command,
        "unset" | "export" | "env" | "set" | "set-item" | "remove-item"
    ) && texts[1..].iter().any(|word| {
        matches!(
            word.as_str(),
            "home" | "userprofile" | "env:home" | "env:userprofile"
        ) || assignment(word)
    })
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return None,
            Component::Normal(value) => normalized.push(value),
        }
    }
    Some(normalized)
}

fn skill_name_from_relative(relative: &Path) -> Option<String> {
    let components = normal_components(relative)?;
    let name = *components.get(components.len().checked_sub(2)?)?;
    (components.last()? == &"SKILL.md" && valid_skill_path_component(name))
        .then(|| name.to_string())
}

fn normal_components(path: &Path) -> Option<Vec<&str>> {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect()
}

fn valid_skill_path_component(name: &str) -> bool {
    !matches!(name, "" | "." | "..")
        && name.len() <= 128
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::skill_names_with_home;

    fn synthetic_home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\home\test")
        } else {
            PathBuf::from("/home/test")
        }
    }

    fn names(command: &str) -> Vec<String> {
        skill_names_with_home(&[command.to_string()], Some(&synthetic_home()))
    }

    #[test]
    fn quote_and_escape_provenance_controls_home_expansion() {
        for command in [
            "cat '$HOME/.codex/skills/single/SKILL.md'",
            r"cat \$HOME/.codex/skills/escaped/SKILL.md",
            r#"cat "\$HOME/.codex/skills/double-escaped/SKILL.md""#,
            r#"cat "~/.codex/skills/quoted-tilde/SKILL.md""#,
            r#"cat "$H""OME/.codex/skills/split-name/SKILL.md""#,
            r#"cat "${HO}""ME/.codex/skills/split-braced/SKILL.md""#,
            r#"cat "$HO"'ME/.codex/skills/mixed/SKILL.md'"#,
            r#"cat "$"'HOME/.codex/skills/mixed-dollar/SKILL.md'"#,
        ] {
            assert!(names(command).is_empty(), "{command}");
        }
        for (command, expected) in [
            ("cat $HOME/.codex/skills/plain/SKILL.md", "plain"),
            (r#"cat "$HOME/.codex/skills/double/SKILL.md""#, "double"),
            (r#"cat "$HOME"/.codex/skills/joined/SKILL.md"#, "joined"),
            ("cat ~/.codex/skills/tilde/SKILL.md", "tilde"),
        ] {
            assert_eq!(names(command), [expected], "{command}");
        }
        let absolute = format!(
            "cat '{}/.codex/skills/absolute/SKILL.md'",
            synthetic_home().display()
        );
        assert_eq!(names(&absolute), ["absolute"], "{absolute}");
    }

    #[test]
    fn every_supported_verb_has_an_unambiguous_positive() {
        for (command, expected) in [
            ("cat -- $HOME/.codex/skills/cat/SKILL.md", "cat"),
            ("sed -n 1p $HOME/.codex/skills/sed/SKILL.md", "sed"),
            ("head -n 2 $HOME/.codex/skills/head/SKILL.md", "head"),
            ("tail -n 2 $HOME/.codex/skills/tail/SKILL.md", "tail"),
            ("less -N $HOME/.codex/skills/less/SKILL.md", "less"),
            ("more -d $HOME/.codex/skills/more/SKILL.md", "more"),
            ("bat --plain $HOME/.codex/skills/bat/SKILL.md", "bat"),
            (
                "Get-Content -Path $HOME/.codex/skills/get/SKILL.md -Raw",
                "get",
            ),
        ] {
            assert_eq!(names(command), [expected], "{command}");
        }
    }

    #[test]
    fn programs_options_and_short_circuits_are_not_paths() {
        for command in [
            "sed '$HOME/.codex/skills/program/SKILL.md'",
            "sed -e '$HOME/.codex/skills/script/SKILL.md' /tmp/input",
            "head -n $HOME/.codex/skills/count/SKILL.md /tmp/input",
            "tail -c $HOME/.codex/skills/bytes/SKILL.md /tmp/input",
            "less -P $HOME/.codex/skills/prompt/SKILL.md /tmp/input",
            "more -n $HOME/.codex/skills/lines/SKILL.md /tmp/input",
            "bat --theme $HOME/.codex/skills/theme/SKILL.md /tmp/input",
            "Get-Content -TotalCount $HOME/.codex/skills/count/SKILL.md /tmp/input",
            "cat --help $HOME/.codex/skills/help/SKILL.md",
            "cat --version $HOME/.codex/skills/version/SKILL.md",
            "cat --unknown $HOME/.codex/skills/unknown/SKILL.md",
        ] {
            assert!(names(command).is_empty(), "{command}");
        }
    }

    #[test]
    fn heredoc_body_is_not_executed_as_shell_source() {
        let command = "printf ignored <<'EOF'\ncat $HOME/.codex/skills/heredoc/SKILL.md\nEOF";
        assert!(names(command).is_empty());
    }

    #[test]
    fn conditional_shell_segments_are_not_assumed_to_execute() {
        for command in [
            "false && cat $HOME/.codex/skills/and/SKILL.md",
            "true || cat $HOME/.codex/skills/or/SKILL.md",
            "false && # still conditional\ncat $HOME/.codex/skills/comment/SKILL.md",
        ] {
            assert!(names(command).is_empty(), "{command}");
        }
    }

    #[test]
    fn home_mutation_makes_all_later_home_expansions_untrusted() {
        for command in [
            "HOME=/tmp; cat $HOME/.codex/skills/assigned/SKILL.md",
            "HOME=/tmp; cat ~/.codex/skills/tilde-assigned/SKILL.md",
            "unset HOME; cat $HOME/.codex/skills/unset/SKILL.md",
            "export USERPROFILE=/tmp; cat $HOME/.codex/skills/profile/SKILL.md",
            r"set USERPROFILE=C:\tmp; Get-Content ~\.codex\skills\windows-tilde\SKILL.md",
        ] {
            assert!(names(command).is_empty(), "{command}");
        }
    }

    #[test]
    fn unquoted_and_quoted_line_continuations_are_removed() {
        for (command, expected) in [
            (
                "cat $HOME/.codex/skills/cont\\\ninued/SKILL.md",
                "continued",
            ),
            (
                "cat \"$HOME/.codex/skills/double\\\ncontinued/SKILL.md\"",
                "doublecontinued",
            ),
        ] {
            assert_eq!(names(command), [expected], "{command}");
        }
    }

    #[test]
    fn windows_home_separators_are_supported() {
        for command in [
            r#"Get-Content "$HOME\.codex\skills\windows\SKILL.md""#,
            r"Get-Content $HOME\.codex\skills\windows\SKILL.md",
        ] {
            assert_eq!(names(command), ["windows"], "{command}");
        }
    }

    #[test]
    fn tail_follow_accepts_an_optional_inline_mode() {
        for mode in ["name", "descriptor"] {
            let command = format!("tail --follow={mode} $HOME/.codex/skills/follow/SKILL.md");
            assert_eq!(names(&command), ["follow"], "{command}");
        }
        assert_eq!(
            names("tail --follow $HOME/.codex/skills/follow/SKILL.md"),
            ["follow"]
        );
        assert!(names("tail --follow=unknown $HOME/.codex/skills/follow/SKILL.md").is_empty());
    }

    #[test]
    fn missing_home_keeps_reads_untrusted_and_ignored() {
        assert!(
            skill_names_with_home(&["cat $HOME/.codex/skills/demo/SKILL.md".to_string()], None)
                .is_empty()
        );
    }
}
