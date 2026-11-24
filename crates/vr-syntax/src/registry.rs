//! Language ids from `languages.toml`: file name → language, embedded-code name → language,
//! and the compiled highlight configuration of each, built on first use.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use vr_config::Languages;

use crate::query::LanguageConfig;
use crate::{Grammar, Status, grammar, inventory};

/// A `languages.toml` problem only vr-syntax can see (the config crate does not know grammars).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryWarning {
    /// AC-13e: the language's files open as plain text.
    GrammarNotCompiledIn {
        language: String,
        grammar: String,
        /// The cargo feature that would compile it in, when it is a known grammar.
        feature: Option<&'static str>,
    },
}

impl std::fmt::Display for RegistryWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryWarning::GrammarNotCompiledIn {
                language,
                grammar,
                feature,
            } => {
                write!(
                    f,
                    "languages.toml: [{language}] uses grammar {grammar:?}, which is not compiled \
                     into this build; its files open as plain text"
                )?;
                if let Some(feature) = feature {
                    write!(f, " (cargo feature {feature:?} is off)")?;
                }
                Ok(())
            }
        }
    }
}

struct Entry {
    id: String,
    grammar: Option<&'static Grammar>,
    /// Lowercase, without the dot.
    extensions: Vec<String>,
    files: Vec<String>,
    /// Lowercase.
    aliases: Vec<String>,
    lsp: Vec<String>,
    config: OnceLock<Result<Arc<LanguageConfig>, String>>,
}

/// Shared by every document (`Arc`); compiled configurations are cached here.
pub struct LanguageRegistry {
    entries: Vec<Entry>,
}

impl std::fmt::Debug for LanguageRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|e| &e.id))
            .finish()
    }
}

impl LanguageRegistry {
    /// The registry for a loaded `languages.toml`, with a warning per language whose grammar is
    /// not compiled in.
    pub fn new(languages: &Languages) -> (LanguageRegistry, Vec<RegistryWarning>) {
        let mut warnings = Vec::new();
        let entries = languages
            .languages
            .iter()
            .map(|def| {
                let g = grammar(&def.grammar);
                if g.is_none() {
                    let feature = inventory().into_iter().find_map(|e| match e.status {
                        Status::FeatureOff { feature } if e.id == def.grammar => Some(feature),
                        _ => None,
                    });
                    warnings.push(RegistryWarning::GrammarNotCompiledIn {
                        language: def.id.clone(),
                        grammar: def.grammar.clone(),
                        feature,
                    });
                }
                Entry {
                    id: def.id.clone(),
                    grammar: g,
                    extensions: def.extensions.iter().map(|e| e.to_lowercase()).collect(),
                    files: def.files.clone(),
                    aliases: def.aliases.iter().map(|a| a.to_lowercase()).collect(),
                    lsp: def.lsp.clone(),
                    config: OnceLock::new(),
                }
            })
            .collect();
        (LanguageRegistry { entries }, warnings)
    }

    /// The registry for the built-in `languages.toml` (tests, previews).
    pub fn builtin() -> LanguageRegistry {
        LanguageRegistry::new(&Languages::default()).0
    }

    /// The language of a file: an exact file name or glob from `files` first (the glob with
    /// the most literal characters wins), then the longest matching extension.
    pub fn language_for_path(&self, path: &Path) -> Option<&str> {
        let name = path.file_name()?.to_str()?;
        let by_file = self
            .entries
            .iter()
            .flat_map(|e| e.files.iter().map(move |f| (e, f)))
            .filter(|(_, f)| glob_match(f, name))
            .max_by_key(|(_, f)| {
                let wild = f.chars().filter(|c| matches!(c, '*' | '?')).count();
                (wild == 0, f.len() - wild)
            });
        if let Some((e, _)) = by_file {
            return Some(&e.id);
        }
        let lower = name.to_lowercase();
        // `.eslintrc.json` has the extensions `eslintrc.json` and `json`, not `.eslintrc...`.
        let stem = lower.strip_prefix('.').unwrap_or(&lower);
        stem.match_indices('.').find_map(|(dot, _)| {
            let ext = &stem[dot + 1..];
            self.entries
                .iter()
                .find(|e| e.extensions.iter().any(|x| x == ext))
                .map(|e| e.id.as_str())
        })
    }

    /// The language named in embedded code (`injection.language`: a Markdown fence's info
    /// string, `<script lang>`): an id, an alias or an extension, case-insensitive; the first
    /// word before `,` or a space counts.
    pub fn language_for_name(&self, name: &str) -> Option<&str> {
        let name = name
            .trim()
            .split([',', ' ', '\t', '{'])
            .next()
            .unwrap_or("")
            .to_lowercase();
        if name.is_empty() {
            return None;
        }
        let find = |pick: &dyn Fn(&Entry) -> bool| self.entries.iter().find(|e| pick(e));
        find(&|e| e.id == name)
            .or_else(|| find(&|e| e.aliases.contains(&name)))
            .or_else(|| find(&|e| e.extensions.contains(&name)))
            .map(|e| e.id.as_str())
    }

    pub fn has_grammar(&self, id: &str) -> bool {
        self.entry(id).is_some_and(|e| e.grammar.is_some())
    }

    /// The language server command of a language (reserved for M8).
    pub fn lsp(&self, id: &str) -> Option<&[String]> {
        self.entry(id).map(|e| e.lsp.as_slice())
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.id.as_str())
    }

    fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The compiled queries of a language, built once (on a worker thread: the first highlight
    /// of a language pays for it, startup does not).
    pub(crate) fn config(&self, id: &str) -> Result<Arc<LanguageConfig>, String> {
        let entry = self
            .entry(id)
            .ok_or_else(|| format!("unknown language {id:?}"))?;
        let grammar = entry
            .grammar
            .ok_or_else(|| format!("grammar of {id:?} is not compiled in"))?;
        entry
            .config
            .get_or_init(|| LanguageConfig::compile(&entry.id, grammar).map(Arc::new))
            .clone()
    }
}

/// `*` matches any run of characters, `?` one character; everything else literally.
fn glob_match(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn glob() {
        assert!(glob_match("*.blade.php", "a.blade.php"));
        assert!(!glob_match("*.blade.php", "a.php"));
        assert!(glob_match("Jake?ile", "Jakefile"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
        assert!(glob_match("Makefile", "Makefile"));
        assert!(!glob_match("Makefile", "makefile"));
    }
}
