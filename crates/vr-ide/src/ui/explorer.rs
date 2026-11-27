//! Explorer panel: the project tree with folders expanded in place (AC-1). Each workspace
//! root is a top-level row (W-2; a missing one is marked); a column on the right separates
//! the panel from the editor.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use vr_fs::{NodeKind, SymlinkTarget};

use crate::app::{App, Focus};
use crate::commands::explorer::Row;
use crate::theme::{ExplorerKind, Theme};

/// Cells of indentation per tree level.
const INDENT: usize = 2;

/// After the name of a row matched by .gitignore (shown with explorer.toggle_ignored).
const IGNORED_MARK: &str = " ·";

/// After the name of a workspace root whose folder is gone (W-2).
const MISSING_MARK: &str = " (missing)";

pub fn render(buf: &mut Buffer, area: Rect, app: &mut App, theme: &Theme) {
    app.explorer.area = area;
    if area.width < 3 || area.height == 0 {
        return;
    }
    let focused = app.focus == Focus::Explorer;
    let ex = &mut app.explorer;
    let height = usize::from(area.height);
    // Keep the cursor visible, unless the wheel moved the view away from it.
    if ex.follow_cursor {
        if ex.cursor < ex.top {
            ex.top = ex.cursor;
        } else if ex.cursor >= ex.top + height {
            ex.top = ex.cursor + 1 - height;
        }
    }
    ex.top = ex.top.min(ex.rows.len().saturating_sub(1));
    ex.drawn_top = ex.top;
    let inner = area.width - 1;
    buf.set_style(Rect::new(area.x, area.y, inner, area.height), theme.text);
    for (row_y, (i, row)) in ex
        .rows
        .iter()
        .enumerate()
        .skip(ex.top)
        .take(height)
        .enumerate()
    {
        let y = area.y + row_y as u16;
        let mut style = if ex.cursor == i && focused {
            theme.list_selected
        } else if ex.cursor == i || ex.selected.contains(&row.path) {
            theme.selection
        } else {
            theme.text
        };
        buf.set_style(Rect::new(area.x, y, inner, 1), style);
        // The focused row keeps the selected-row foreground: the kind colours are only checked
        // against the editor and selection backgrounds.
        if !(ex.cursor == i && focused) {
            let kind = kind_of(&row.name(), row.kind.is_expandable());
            if let Some(fg) = theme.explorer(kind).fg {
                style = style.fg(fg);
            }
            style = style.add_modifier(theme.explorer(kind).add_modifier);
        }
        if row.ignored
            || row.missing
            || matches!(
                row.kind,
                NodeKind::Symlink(SymlinkTarget::Broken | SymlinkTarget::Loop)
            )
        {
            style = style.patch(theme.dim);
        }
        if row.depth == 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        let text = format!(
            "{}{}{}",
            " ".repeat(INDENT * row.depth),
            icon(row, ex.expanded.contains(&row.path)),
            row.name()
        );
        // Ignored rows and missing roots are also marked in text, so the flag shows without
        // colours; the marker is kept when a long name is cut.
        let marker = if row.missing {
            MISSING_MARK
        } else if row.ignored {
            IGNORED_MARK
        } else {
            ""
        };
        let room = usize::from(inner).saturating_sub(marker.chars().count());
        let shown = format!("{}{marker}", fit(&text, room));
        buf.set_stringn(area.x, y, shown, usize::from(inner), style);
    }
    let sep = if focused { theme.focus } else { theme.dim };
    for y in area.y..area.y + area.height {
        buf.set_string(area.x + inner, y, "│", sep);
    }
}

fn icon(row: &Row, expanded: bool) -> &'static str {
    if row.missing {
        return "✗ ";
    }
    match row.kind {
        k if k.is_expandable() && expanded => "▾ ",
        k if k.is_expandable() => "▸ ",
        NodeKind::Symlink(SymlinkTarget::Loop) => "↻ ",
        NodeKind::Symlink(SymlinkTarget::Broken) => "✗ ",
        _ => "  ",
    }
}

/// `text` cut to `width` cells, ending in "…" when it did not fit.
pub(crate) fn fit(text: &str, width: usize) -> String {
    use ratatui::text::Span;
    if Span::raw(text).width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = Span::raw(ch.to_string()).width();
        if used + w + 1 > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

/// The kind of an explorer row from its name (extension, case-insensitive) and whether it is
/// a folder (AC-F5).
pub fn kind_of(name: &str, is_dir: bool) -> ExplorerKind {
    if is_dir {
        return ExplorerKind::Folder;
    }
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "package-lock.json" | "go.sum" | "npm-shrinkwrap.json" => return ExplorerKind::Generated,
        "makefile" | "dockerfile" | "justfile" | "cmakelists.txt" | "gemfile" | "rakefile"
        | ".gitignore" | ".gitattributes" | ".gitmodules" | ".dockerignore" | ".editorconfig" => {
            return ExplorerKind::Config;
        }
        "readme" | "license" | "licence" | "changelog" | "authors" | "notice" | "copying" => {
            return ExplorerKind::Docs;
        }
        _ => {}
    }
    if lower.ends_with(".min.js") || lower.ends_with(".min.css") {
        return ExplorerKind::Generated;
    }
    // A leading dot is not an extension separator: ".bashrc" has none.
    let ext = match lower.rfind('.') {
        Some(0) | None => "",
        Some(i) => &lower[i + 1..],
    };
    match ext {
        "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "go" | "java" | "kt" | "kts"
        | "swift" | "cs" | "zig" | "dart" | "scala" | "hs" | "ml" | "m" | "mm" | "nim" => {
            ExplorerKind::CodeNative
        }
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "vue" | "svelte" | "php" | "py" | "rb"
        | "pl" | "lua" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "html" | "htm" | "css"
        | "scss" | "sass" | "less" | "twig" | "blade" | "sql" | "r" | "ex" | "exs" => {
            ExplorerKind::CodeScript
        }
        "toml" | "json" | "jsonc" | "json5" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "xml"
        | "env" | "csv" | "tsv" | "properties" | "dtd" => ExplorerKind::Config,
        "md" | "markdown" | "txt" | "rst" | "adoc" | "org" | "pdf" | "doc" | "docx" | "odt"
        | "rtf" | "log" => ExplorerKind::Docs,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "tiff" | "avif"
        | "mp3" | "wav" | "flac" | "ogg" | "mp4" | "mkv" | "webm" | "mov" | "avi" | "ttf"
        | "otf" | "woff" | "woff2" => ExplorerKind::Media,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "jar" | "war"
        | "exe" | "dll" | "so" | "dylib" | "a" | "o" | "bin" | "wasm" | "deb" | "rpm" | "iso" => {
            ExplorerKind::Archive
        }
        "lock" | "map" | "lockb" | "pyc" | "class" => ExplorerKind::Generated,
        _ if lower.starts_with('.') => ExplorerKind::Hidden,
        _ => ExplorerKind::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ExplorerKind::*;

    #[test]
    fn kinds_by_extension_name_and_case() {
        for (name, dir, want) in [
            ("src", true, Folder),
            (".github", true, Folder),
            ("main.rs", false, CodeNative),
            ("MAIN.RS", false, CodeNative),
            ("app.Tsx", false, CodeScript),
            ("index.php", false, CodeScript),
            ("Cargo.toml", false, Config),
            ("composer.json", false, Config),
            ("README.md", false, Docs),
            ("LICENSE", false, Docs),
            ("logo.PNG", false, Media),
            ("release.tar.gz", false, Archive),
            ("tool.exe", false, Archive),
            ("Cargo.lock", false, Generated),
            ("package-lock.json", false, Generated),
            ("yarn.lock", false, Generated),
            ("app.min.js", false, Generated),
            ("Makefile", false, Config),
            ("Dockerfile", false, Config),
            (".gitignore", false, Config),
            (".bashrc", false, Hidden),
            (".env.local", false, Hidden),
            ("notes", false, Plain),
            ("weird.zzz", false, Plain),
            ("trailing.", false, Plain),
        ] {
            assert_eq!(kind_of(name, dir), want, "{name}");
        }
    }
}
