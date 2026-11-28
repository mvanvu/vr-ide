//! Clipboard access: the system clipboard through arboard when a display is reachable,
//! otherwise OSC 52 (the terminal sets its host's clipboard, which also works over SSH).

use std::io::Write;
use std::sync::{Arc, Mutex};

pub trait Clipboard {
    fn set(&mut self, text: &str) -> Result<(), String>;
    fn get(&mut self) -> Option<String>;
    /// Short name for notices and logs.
    fn kind(&self) -> &'static str;
}

/// In-memory clipboard; clones share the content (tests keep a handle to inspect it).
#[derive(Debug, Clone, Default)]
pub struct MemoryClipboard(Arc<Mutex<Option<String>>>);

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contents(&self) -> Option<String> {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Clipboard for MemoryClipboard {
    fn set(&mut self, text: &str) -> Result<(), String> {
        *self.lock() = Some(text.to_owned());
        Ok(())
    }

    fn get(&mut self) -> Option<String> {
        self.contents()
    }

    fn kind(&self) -> &'static str {
        "memory"
    }
}

/// Writes OSC 52 sequences to `out`. Terminals rarely allow reading the clipboard back,
/// so paste returns the last text copied from this editor.
pub struct Osc52Clipboard<W: Write> {
    out: W,
    last: Option<String>,
}

impl<W: Write> Osc52Clipboard<W> {
    pub fn new(out: W) -> Self {
        Self { out, last: None }
    }

    pub fn into_inner(self) -> W {
        self.out
    }
}

impl<W: Write> Clipboard for Osc52Clipboard<W> {
    fn set(&mut self, text: &str) -> Result<(), String> {
        self.last = Some(text.to_owned());
        self.out
            .write_all(osc52_sequence(text).as_bytes())
            .and_then(|()| self.out.flush())
            .map_err(|e| e.to_string())
    }

    fn get(&mut self) -> Option<String> {
        self.last.clone()
    }

    fn kind(&self) -> &'static str {
        "osc52"
    }
}

/// System clipboard with OSC 52 as the fallback when arboard fails at runtime.
pub struct SystemClipboard {
    inner: arboard::Clipboard,
    fallback: Osc52Clipboard<std::io::Stdout>,
}

impl Clipboard for SystemClipboard {
    fn set(&mut self, text: &str) -> Result<(), String> {
        match self.inner.set_text(text.to_owned()) {
            Ok(()) => {
                self.fallback.last = Some(text.to_owned());
                Ok(())
            }
            Err(e) => {
                tracing::warn!(error = %e, "system clipboard failed; using OSC 52");
                self.fallback.set(text)
            }
        }
    }

    fn get(&mut self) -> Option<String> {
        self.inner.get_text().ok().or_else(|| self.fallback.get())
    }

    fn kind(&self) -> &'static str {
        "system"
    }
}

/// The clipboard for a real terminal session.
pub fn system() -> Box<dyn Clipboard> {
    let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()));
    if has_display && let Ok(inner) = arboard::Clipboard::new() {
        return Box::new(SystemClipboard {
            inner,
            fallback: Osc52Clipboard::new(std::io::stdout()),
        });
    }
    Box::new(Osc52Clipboard::new(std::io::stdout()))
}

/// The system clipboard in the native window: no OSC 52 fallback (there is no terminal to
/// read it). When the system clipboard cannot be reached, copies stay inside vr-ide.
pub struct WindowClipboard {
    inner: Option<arboard::Clipboard>,
    last: Option<String>,
}

impl WindowClipboard {
    pub fn new() -> Self {
        Self {
            inner: arboard::Clipboard::new().ok(),
            last: None,
        }
    }
}

impl Default for WindowClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for WindowClipboard {
    fn set(&mut self, text: &str) -> Result<(), String> {
        self.last = Some(text.to_owned());
        match &mut self.inner {
            Some(c) => c.set_text(text.to_owned()).map_err(|e| e.to_string()),
            None => Err("the system clipboard is not available; copied inside vr-ide only".into()),
        }
    }

    fn get(&mut self) -> Option<String> {
        self.inner
            .as_mut()
            .and_then(|c| c.get_text().ok())
            .or_else(|| self.last.clone())
    }

    fn kind(&self) -> &'static str {
        "system"
    }
}

/// `ESC ] 52 ; c ; <base64> BEL`.
pub fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// Standard base64 with padding (RFC 4648).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, want) in cases {
            assert_eq!(base64(input.as_bytes()), want, "{input:?}");
        }
    }

    #[test]
    fn osc52_fallback_writes_sequence_and_pastes_last_copy() {
        let mut c = Osc52Clipboard::new(Vec::new());
        c.set("Xin chào").unwrap();
        assert_eq!(c.get().as_deref(), Some("Xin chào"));
        let out = String::from_utf8(c.into_inner()).unwrap();
        assert_eq!(
            out,
            format!("\x1b]52;c;{}\x07", base64("Xin chào".as_bytes()))
        );
    }
}
