//! The `/ssh` argument line (D5, SEC-1..SEC-3): the text typed after `/ssh ` as it would be
//! typed after `ssh ` in a terminal, split into words like a shell would split it but without
//! a shell, then read with ssh's own option grammar to find the destination.
//!
//! Nothing here runs anything or touches the disk; [`Line`] is the only way a line becomes
//! the words of an ssh argv, and a line that does not split cleanly or has no valid
//! destination never becomes one.

use std::path::{Path, PathBuf};

use super::Target;

/// Longest line accepted, in bytes.
pub const MAX_BYTES: usize = 4 * 1024;
/// Most words a line may split into.
pub const MAX_WORDS: usize = 128;

/// OpenSSH 9.6 `ssh` option letters; a letter followed by `:` takes a value.
const OPTSTRING: &str = "1246ab:c:e:fgi:kl:m:no:p:qstvxAB:CD:E:F:GI:J:KL:MNO:P:Q:R:S:TVw:W:XYy";

/// The option letter naming the user's own ssh config file, which can run local programs
/// (`Match exec`, `ProxyCommand`): a recent line with one is read before it runs (SEC-5).
pub const CONFIG_FILE: char = 'F';

/// Option letters whose value is a local path: identity, config, log file, control socket.
const PATH_OPTIONS: &[char] = &['i', 'F', 'E', 'S'];

/// Options that make vr-ide leave its own `-o` prefix out (SEC-3): with them the session is
/// a connection-sharing client or master, or ssh only prints something and exits.
const SHARING_OPTIONS: &[char] = &['M', 'S', 'O', 'G', 'V', 'Q', 'f'];

/// `-o` keywords (any part of any word, case-insensitive) whose value vr-ide's prefix would
/// override, since ssh keeps the first value it is given (SEC-3).
const OWN_KEYWORDS: &[&str] = &[
    "localcommand",
    "controlmaster",
    "controlpath",
    "controlpersist",
];

/// Why a line was refused. Each notice is fixed text: what was typed is never echoed, since
/// it may hold a password or a remote command's secret (SEC-1, SEC-S9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineError {
    UnclosedSingle,
    UnclosedDouble,
    TrailingBackslash,
    Control,
    TooLong,
    TooManyWords,
    Empty,
    /// An option ssh does not know, or one whose value is missing.
    BadOption,
    /// No destination word, or one that is not a valid target (SEC-2).
    NoDestination,
}

impl LineError {
    pub fn notice(self) -> &'static str {
        match self {
            LineError::UnclosedSingle => "/ssh: unclosed ' (quote paths with spaces, or use \\ )",
            LineError::UnclosedDouble => "/ssh: unclosed \" (quote paths with spaces, or use \\ )",
            LineError::TrailingBackslash => "/ssh: a \\ at the end escapes nothing",
            LineError::Control => "/ssh: the line must be one line without control characters",
            LineError::TooLong => "/ssh: the line is longer than 4 KiB",
            LineError::TooManyWords => "/ssh: the line has more than 128 words",
            LineError::Empty => "/ssh: nothing to run",
            LineError::BadOption => "/ssh: an option ssh does not know, or one without its value",
            LineError::NoDestination => super::NOT_VALID,
        }
    }
}

/// One word of a line with where its text starts in the line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    text: String,
    /// Byte offset of the word's first character in the line.
    start: usize,
}

/// Splits `line` into words: single quotes keep everything literally, double quotes keep
/// everything but `\"`, `\\`, `` \` `` and `\$` (which lose their backslash), a backslash
/// outside quotes keeps the next character literally. `$`, backticks, globs, `;`, `|`, `&`
/// and redirections are ordinary characters. An unquoted `~` that is a whole word or starts
/// `~/` names `home` (never `~user`). `lenient`: an unclosed quote or a trailing backslash
/// ends the last word instead of failing (completion of a line being typed).
fn scan(line: &str, home: Option<&Path>, lenient: bool) -> Result<Vec<Word>, LineError> {
    let mut words = Vec::new();
    let mut cur: Option<Word> = None;
    let mut chars = line.char_indices().peekable();
    // At the first character of a word, nothing quoted yet: where `~` may expand.
    let mut at_start = true;
    while let Some((i, c)) = chars.next() {
        let word = cur.get_or_insert_with(|| Word {
            text: String::new(),
            start: i,
        });
        match c {
            ' ' => {
                if let Some(w) = cur.take()
                    && !(w.text.is_empty() && w.start == i)
                {
                    words.push(w);
                }
                at_start = true;
                continue;
            }
            '\'' => {
                let mut closed = false;
                for (_, q) in chars.by_ref() {
                    if q == '\'' {
                        closed = true;
                        break;
                    }
                    word.text.push(q);
                }
                if !closed && !lenient {
                    return Err(LineError::UnclosedSingle);
                }
            }
            '"' => {
                let mut closed = false;
                while let Some((_, q)) = chars.next() {
                    match q {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => match chars.peek() {
                            Some(&(_, e @ ('"' | '\\' | '`' | '$'))) => {
                                word.text.push(e);
                                chars.next();
                            }
                            _ => word.text.push('\\'),
                        },
                        _ => word.text.push(q),
                    }
                }
                if !closed && !lenient {
                    return Err(LineError::UnclosedDouble);
                }
            }
            '\\' => match chars.next() {
                Some((_, e)) => word.text.push(e),
                None if lenient => {}
                None => return Err(LineError::TrailingBackslash),
            },
            '~' if at_start
                && matches!(chars.peek(), None | Some(&(_, ' ' | '/')))
                && home.is_some() =>
            {
                if let Some(h) = home {
                    word.text.push_str(&h.to_string_lossy());
                }
            }
            _ => word.text.push(c),
        }
        at_start = false;
    }
    if let Some(w) = cur.take() {
        words.push(w);
    }
    Ok(words)
}

/// The words of `line` (SEC-1), or why it is refused. Limits are checked before anything
/// else is done with the text.
pub fn split(line: &str, home: Option<&Path>) -> Result<Vec<String>, LineError> {
    if line.len() > MAX_BYTES {
        return Err(LineError::TooLong);
    }
    if line.chars().any(char::is_control) {
        return Err(LineError::Control);
    }
    let words: Vec<String> = scan(line, home, false)?
        .into_iter()
        .map(|w| w.text)
        .collect();
    if words.len() > MAX_WORDS {
        return Err(LineError::TooManyWords);
    }
    if words.is_empty() {
        return Err(LineError::Empty);
    }
    Ok(words)
}

/// Whether ssh's option `c` takes a value; `None`: not an ssh option.
fn takes_value(c: char) -> Option<bool> {
    if c == ':' {
        return None;
    }
    let at = OPTSTRING.find(c)?;
    Some(OPTSTRING[at + c.len_utf8()..].starts_with(':'))
}

/// One option as ssh reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opt {
    pub letter: char,
    pub value: Option<String>,
}

/// The words read the way ssh reads its argv: options (bundled `-vp22`, attached `-p22`,
/// `-oKey=v`), then the destination, then options again until the first word that is not
/// one (or `--`), then the remote command.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Parsed {
    opts: Vec<Opt>,
    /// Index of the destination word.
    dest: Option<usize>,
}

/// Where a walk over the words ended: the state the next word would be read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// An option, or the destination when no destination was seen yet.
    Option { dest_seen: bool },
    /// The value of option `letter`.
    Value(char),
    /// A word of the remote command (or after `--` with the destination seen).
    Command,
    /// The destination (after `--`).
    Destination,
}

/// ssh's getopt over `words`: `Err` for an unknown option or a missing value (ssh would
/// print its usage), else the options, the destination and the state after the last word.
fn walk(words: &[&str]) -> (Result<Parsed, LineError>, Next) {
    let mut opts = Vec::new();
    let mut dest = None;
    let mut terminated = false;
    let mut i = 0;
    let n = words.len();
    loop {
        while i < n {
            let w = words[i];
            if w == "--" {
                terminated = true;
                i += 1;
                break;
            }
            if !w.starts_with('-') || w == "-" {
                break;
            }
            let body = &w[1..];
            for (k, c) in body.char_indices() {
                match takes_value(c) {
                    None => return (Err(LineError::BadOption), Next::Command),
                    Some(false) => opts.push(Opt {
                        letter: c,
                        value: None,
                    }),
                    Some(true) => {
                        let rest = &body[k + c.len_utf8()..];
                        let value = if !rest.is_empty() {
                            rest.to_string()
                        } else if i + 1 < n {
                            i += 1;
                            words[i].to_string()
                        } else {
                            return (Err(LineError::BadOption), Next::Value(c));
                        };
                        opts.push(Opt {
                            letter: c,
                            value: Some(value),
                        });
                        break;
                    }
                }
            }
            i += 1;
        }
        if dest.is_none() && i < n {
            dest = Some(i);
            i += 1;
            if !terminated && i < n {
                continue;
            }
        }
        break;
    }
    let next = match (dest, terminated) {
        (None, true) => Next::Destination,
        (None, false) => Next::Option { dest_seen: false },
        (Some(_), true) => Next::Command,
        (Some(_), false) if i < n => Next::Command,
        (Some(_), false) => Next::Option { dest_seen: true },
    };
    (Ok(Parsed { opts, dest }), next)
}

/// The destination word as ssh takes it: `[user@]host`, `[user@]v6-address` or
/// `ssh://[user@]host[:port]`, validated by [`Target::parse`] (a word that starts with `-` is
/// never one). A `host:port` outside the URI form is not something ssh accepts.
pub fn destination(word: &str) -> Option<Target> {
    if word.starts_with("ssh://") {
        return Target::parse(word);
    }
    if let Some(t) = Target::parse(word) {
        return (t.port.is_none() && !t.v6).then_some(t);
    }
    // A bare IPv6 address, as ssh takes it without brackets.
    let (user, addr) = match word.rsplit_once('@') {
        Some((u, a)) => (Some(u), a),
        None => (None, word),
    };
    let bracketed = match user {
        Some(u) => format!("{u}@[{addr}]"),
        None => format!("[{addr}]"),
    };
    let t = Target::parse(&bracketed)?;
    Some(Target {
        text: word.to_string(),
        ..t
    })
}

/// A checked `/ssh` argument line: the words ssh gets after vr-ide's own prefix, and the
/// destination the label is made of (SEC-1, SEC-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The line as typed (trimmed), or the migrated form of an old `host:port` target.
    text: String,
    words: Vec<String>,
    target: Target,
    opts: Vec<Opt>,
}

impl Line {
    /// The line typed after `/ssh `. A single old-style target with a port
    /// (`bob@h:2222`, `u@[::1]:22`) means what it meant before: `-p 2222 bob@h`.
    pub fn parse(text: &str, home: Option<&Path>) -> Result<Line, LineError> {
        let text = text.trim_matches(' ');
        let migrated = migrate(text);
        let text = migrated.as_deref().unwrap_or(text);
        let words = split(text, home)?;
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        let parsed = walk(&refs).0?;
        let target = parsed
            .dest
            .and_then(|d| destination(&words[d]))
            .ok_or(LineError::NoDestination)?;
        Ok(Line {
            text: text.to_string(),
            words,
            target,
            opts: parsed.opts,
        })
    }

    /// The line as stored in the recent list and listed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The words, in order, as ssh gets them.
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// The validated destination: the source of the tab label and the status row.
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The user's own words set what vr-ide's prefix would set, or make the session share a
    /// connection or only print something: the prefix is left out entirely (SEC-3).
    pub fn keeps_own_options(&self) -> bool {
        self.opts
            .iter()
            .any(|o| SHARING_OPTIONS.contains(&o.letter))
            || self.words.iter().any(|w| {
                let lower = w.to_ascii_lowercase();
                OWN_KEYWORDS.iter().any(|k| lower.contains(k))
            })
    }

    /// The name of the first option that makes ssh run a local program, for the hint shown
    /// before a recent line is run again (SEC-5).
    pub fn local_command(&self) -> Option<String> {
        local_command_word(&self.words)
    }
}

/// An old recent-list target `[user@]host:port` / `[user@][v6]:port` as an argument line.
fn migrate(text: &str) -> Option<String> {
    if text.contains(' ') || text.starts_with("ssh://") {
        return None;
    }
    let t = Target::parse(text)?;
    let port = t.port?;
    let host = match &t.user {
        Some(u) => format!("{u}@{}", t.host),
        None => t.host.clone(),
    };
    Some(format!("-p {port} {host}"))
}

/// The name of the first thing in `words` that can make ssh run a local program: an `-I`
/// anywhere the getopt walk finds one (`-I x`, `-vI x`, `-qIx`), a `…Command`,
/// `PKCS11Provider` or `SecurityKeyProvider` word (case-insensitive), or `-F` with a file
/// (a config file can hold `Match exec` or `ProxyCommand`; `-F none` reads none).
pub fn local_command_word(words: &[String]) -> Option<String> {
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    let parsed = walk(&refs).0.ok();
    // A line that does not parse never runs; its words are still read as options would be.
    let i_option = match &parsed {
        Some(p) => p.opts.iter().any(|o| o.letter == 'I'),
        None => words.iter().any(|w| {
            w.starts_with('-') && !w.starts_with("--") && option_letters(w).contains(&'I')
        }),
    };
    if i_option {
        return Some("-I".to_string());
    }
    let keyword = words.iter().find_map(|w| {
        let lower = w.to_ascii_lowercase();
        let hit = lower.contains("command")
            || lower.contains("pkcs11provider")
            || lower.contains("securitykeyprovider");
        if !hit {
            return None;
        }
        let key = w.strip_prefix("-o").unwrap_or(w);
        let name: String = key
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .take(32)
            .collect();
        Some(if name.is_empty() {
            "-o".to_string()
        } else {
            name
        })
    });
    if keyword.is_some() {
        return keyword;
    }
    parsed?
        .opts
        .iter()
        .any(|o| {
            o.letter == CONFIG_FILE
                && o.value
                    .as_deref()
                    .is_some_and(|v| !v.eq_ignore_ascii_case("none"))
        })
        .then(|| format!("-{CONFIG_FILE}"))
}

/// The option letters of one `-…` word up to the first one that takes a value.
fn option_letters(word: &str) -> Vec<char> {
    let mut out = Vec::new();
    for c in word[1..].chars() {
        out.push(c);
        if takes_value(c) != Some(false) {
            break;
        }
    }
    out
}

/// An `-o` value `Key=value` / `Key value` whose value is a local path; the byte offset of
/// the value within it.
fn path_keyword(opt: &str) -> Option<usize> {
    let at = opt.find(['=', ' '])?;
    let key = opt[..at].to_ascii_lowercase();
    (key.ends_with("file") || key == "controlpath" || key == "identityagent").then_some(at + 1)
}

/// Where the cursor of an `/ssh` argument line is, for Tab completion (T4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgPos {
    /// The value of an option that names a local path (`-i`, `-F`, `-E`, `-S`,
    /// `-o IdentityFile=` and other `…File=`); `start`: byte offset in the line where the
    /// path text begins (after `-i`, `=` or the opening quote).
    Path { start: usize },
    /// The destination word; `start`: where it begins.
    Destination { start: usize },
    /// Anything else (an option letter, a value that is not a path, the remote command).
    Other,
}

/// What the text before `cursor` (a byte offset into `line`, the argument line without
/// `/ssh `) is being typed as, read with ssh's grammar. An unclosed quote or a trailing
/// backslash is allowed: the line is still being typed. Pure; reads nothing.
pub fn classify(line: &str, cursor: usize) -> ArgPos {
    let Some(before) = line.get(..cursor.min(line.len())) else {
        return ArgPos::Other;
    };
    let Ok(mut words) = scan(before, None, true) else {
        return ArgPos::Other;
    };
    // A word is being typed unless the text ends in a separating space.
    let typing = !before.is_empty() && !ends_with_separator(before);
    let current = if typing {
        words.pop().unwrap_or(Word {
            text: String::new(),
            start: before.len(),
        })
    } else {
        Word {
            text: String::new(),
            start: before.len(),
        }
    };
    let refs: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    let (result, next) = walk(&refs);
    let next = match result {
        Ok(_) => next,
        Err(_) => match next {
            Next::Value(c) => Next::Value(c),
            _ => return ArgPos::Other,
        },
    };
    // Offsets into the current word are taken from its unquoted text: a quote before the
    // value shifts them, which completion reads as part of what was typed.
    let at = |skip_bytes: usize| current.start + skip_bytes;
    match next {
        Next::Value(c) if PATH_OPTIONS.contains(&c) => ArgPos::Path { start: at(0) },
        Next::Value('o') => match path_keyword(&current.text) {
            Some(off) => ArgPos::Path { start: at(off) },
            None => ArgPos::Other,
        },
        Next::Value(_) | Next::Command => ArgPos::Other,
        Next::Destination => ArgPos::Destination { start: at(0) },
        Next::Option { dest_seen } => {
            let t = current.text.as_str();
            if let Some(body) = t.strip_prefix('-')
                && t != "-"
            {
                // The first letter that takes a value decides what the rest of the word is.
                for (k, c) in body.char_indices() {
                    match takes_value(c) {
                        Some(true) => {
                            let off = 1 + k + c.len_utf8();
                            if PATH_OPTIONS.contains(&c) {
                                return ArgPos::Path { start: at(off) };
                            }
                            if c == 'o' {
                                return path_keyword(&t[off..]).map_or(ArgPos::Other, |p| {
                                    ArgPos::Path { start: at(off + p) }
                                });
                            }
                            return ArgPos::Other;
                        }
                        Some(false) => {}
                        None => return ArgPos::Other,
                    }
                }
                ArgPos::Other
            } else if dest_seen {
                ArgPos::Other
            } else {
                ArgPos::Destination { start: at(0) }
            }
        }
    }
}

/// `text` ends in a space that separates words (not one escaped or inside quotes).
fn ends_with_separator(text: &str) -> bool {
    if !text.ends_with(' ') {
        return false;
    }
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut last_sep = false;
    for c in text.chars() {
        last_sep = false;
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, c) {
            (None, '\\') => escaped = true,
            (Some('"'), '\\') => escaped = true,
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, ' ') => last_sep = true,
            _ => {}
        }
    }
    last_sep
}

/// A word as it would be typed into a shell to mean itself: bare when it holds only safe
/// characters, else in single quotes.
pub fn quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word.chars().all(|c| {
            c.is_alphanumeric()
                || matches!(c, '-' | '_' | '.' | '/' | '@' | ':' | ',' | '=' | '+' | '%')
        });
    if safe {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// The command a line runs, as shown before it runs (`runs: ssh …`): each word quoted as a
/// shell would need it, and the relative path values of `-i`/`-F`/`-E`/`-S`/`-o …File=`
/// shown joined to `base`, the folder ssh runs in (it resolves them there itself).
pub fn shown_command(words: &[String], base: &Path) -> String {
    shown_command_at(words, base).0
}

/// [`shown_command`] and the byte offset of the destination word in it.
pub fn shown_command_at(words: &[String], base: &Path) -> (String, Option<usize>) {
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    let join = |v: &str| -> String {
        // `none` is ssh's word for no file at all.
        if v.is_empty() || v.starts_with('/') || v.starts_with('~') || v == "none" {
            v.to_string()
        } else {
            base.join(v).to_string_lossy().into_owned()
        }
    };
    let mut out = vec!["ssh".to_string()];
    let mut i = 0;
    // Only the option part of the line is looked at: the remote command is shown as typed.
    let (parsed, _) = walk(&refs);
    let dest = parsed.ok().and_then(|p| p.dest);
    let opt_end = dest.map_or(words.len(), |d| option_end(&refs, d));
    let mut dest_out = None;
    while i < words.len() {
        let w = words[i].as_str();
        let mut shown = w.to_string();
        if i < opt_end && w.starts_with('-') && w != "--" {
            for (k, c) in w[1..].char_indices() {
                match takes_value(c) {
                    Some(true) => {
                        let off = 1 + k + c.len_utf8();
                        let attached = &w[off..];
                        let is_path = |v: &str| {
                            if PATH_OPTIONS.contains(&c) {
                                Some(join(v))
                            } else if c == 'o' {
                                path_keyword(v).map(|p| format!("{}{}", &v[..p], join(&v[p..])))
                            } else {
                                None
                            }
                        };
                        if !attached.is_empty() {
                            if let Some(v) = is_path(attached) {
                                shown = format!("{}{v}", &w[..off]);
                            }
                        } else if i + 1 < words.len() {
                            out.push(quote(w));
                            i += 1;
                            let v = words[i].as_str();
                            shown = is_path(v).unwrap_or_else(|| v.to_string());
                        }
                        break;
                    }
                    Some(false) => {}
                    None => break,
                }
            }
        }
        if Some(i) == dest {
            dest_out = Some(out.len());
        }
        out.push(quote(&shown));
        i += 1;
    }
    let at = dest_out.map(|n| out[..n].iter().map(|w| w.len() + 1).sum());
    (out.join(" "), at)
}

/// Index of the first remote-command word of `words` whose destination is at `dest`.
fn option_end(words: &[&str], dest: usize) -> usize {
    let mut i = dest + 1;
    while i < words.len() {
        let w = words[i];
        if w == "--" {
            return i + 1;
        }
        if !w.starts_with('-') || w == "-" {
            return i;
        }
        for (k, c) in w[1..].char_indices() {
            if takes_value(c) == Some(true) {
                if 1 + k + c.len_utf8() == w.len() {
                    i += 1;
                }
                break;
            }
        }
        i += 1;
    }
    words.len()
}

/// `text` (a shown command) in lines of at most `width` cells: broken at spaces, or after a
/// `/` inside a long path, never inside a name; continuation lines start with `indent`
/// spaces.
pub fn wrap_command(text: &str, width: usize, indent: usize) -> Vec<String> {
    let width = width.max(indent + 8);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    // Pieces that may start a line: words, and the parts of a word after each `/`.
    let mut pieces: Vec<(String, bool)> = Vec::new();
    for (wi, word) in text.split(' ').enumerate() {
        let mut first = true;
        let mut rest = word;
        while !rest.is_empty() {
            let cut = rest.find('/').map_or(rest.len(), |p| p + 1);
            pieces.push((rest[..cut].to_string(), first && wi > 0));
            first = false;
            rest = &rest[cut..];
        }
    }
    for (piece, spaced) in pieces {
        let sep = if spaced && !line.trim().is_empty() {
            " "
        } else {
            ""
        };
        if line.trim().is_empty() || line.cells() + sep.len() + piece.cells() <= width {
            line.push_str(sep);
            line.push_str(&piece);
        } else {
            lines.push(std::mem::take(&mut line));
            line = " ".repeat(indent);
            line.push_str(&piece);
        }
    }
    if !line.trim().is_empty() {
        lines.push(line);
    }
    lines
}

/// A listed line cut to `cells` display cells keeping its start (the first option) and its
/// destination visible: `-o ProxyCommand="ssh -W %h…" deploy@db1`. `None` when the line does
/// not parse (the caller cuts it at the end).
pub fn cut_middle(line: &str, cells: usize) -> Option<String> {
    if line.cells() <= cells {
        return Some(line.to_string());
    }
    let words = scan(line, None, false).ok()?;
    let refs: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    let dest = walk(&refs).0.ok()?.dest?;
    let start = words[dest].start;
    // The destination as written (up to the next space that separates words).
    let end = words
        .get(dest + 1)
        .map_or(line.len(), |w| line[..w.start].trim_end().len());
    let tail = &line[start..end];
    let tail = if end < line.len() {
        format!(" {tail}…")
    } else {
        format!(" {tail}")
    };
    let head_room = cells.checked_sub(tail.cells() + 1)?;
    let mut head = String::new();
    for c in line[..start].trim_end().chars() {
        let mut s = head.clone();
        s.push(c);
        if s.cells() > head_room {
            break;
        }
        head = s;
    }
    Some(format!("{head}…{tail}"))
}

/// The folder a line's relative paths are taken from, made canonical; `None` when it is
/// gone (SEC-4: never a silent fallback to another folder).
pub fn canonical_base(root: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(root).ok().filter(|p| p.is_dir())
}

/// Display cells of text.
trait Cells {
    fn cells(&self) -> usize;
}

impl Cells for str {
    fn cells(&self) -> usize {
        ratatui::text::Span::raw(self).width()
    }
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
