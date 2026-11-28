//! Fonts: family resolution, cell metrics and the glyph cache.

use std::collections::HashMap;

use cosmic_text::fontdb;
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
    Wrap,
};

/// Monospace families tried, in order, when the generic `monospace` family of the platform does
/// not resolve to an installed face (fontconfig is only read on Linux / BSD).
const MONOSPACE_CANDIDATES: [&str; 9] = [
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Cascadia Mono",
    "Consolas",
    "Menlo",
    "SF Mono",
    "Ubuntu Mono",
    "Courier New",
];

/// Longest grapheme cluster shaped as is; longer ones (runs of combining marks) are cut, so
/// one cell can never cost unbounded shaping time or cache memory.
const MAX_CLUSTER_BYTES: usize = 64;

/// Non-ASCII clusters kept before the cache is emptied and refilled with what is on screen.
const CACHE_LIMIT: usize = 4096;

/// Largest font size accepted, in points.
const MAX_SIZE_PT: f32 = 200.0;

/// Pixels per point at scale 1 (96 dpi).
const PX_PER_PT: f32 = 96.0 / 72.0;

/// The font the grid is drawn with.
#[derive(Debug, Clone, PartialEq)]
pub struct FontConfig {
    /// Family name; `None` is the platform's monospace family.
    pub family: Option<String>,
    /// Size in points (at scale 1, 1 pt = 4/3 px).
    pub size_pt: f32,
    /// Line height as a factor of the font size.
    pub line_height: f32,
    /// HiDPI scale factor of the window.
    pub scale: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: None,
            // The settings' default size.
            size_pt: 11.0,
            line_height: 1.2,
            scale: 1.0,
        }
    }
}

/// Why a font configuration could not be used; the font set keeps (or falls back to) a working
/// one, so the caller shows this as a warning.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FontError {
    #[error("font family \"{0}\" is not installed; using the default monospace font")]
    FamilyNotFound(String),
    #[error("font size {0} is not a positive number; using the default size")]
    InvalidSize(f32),
    #[error("line height {0} is not between 0.5 and 4; using the default line height")]
    InvalidLineHeight(f32),
    #[error("scale factor {0} is not a positive number; using 1")]
    InvalidScale(f32),
}

/// Bold and italic, the style parts that change the glyph shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GlyphStyle {
    pub bold: bool,
    pub italic: bool,
}

impl GlyphStyle {
    const fn slot(self) -> usize {
        self.bold as usize | (self.italic as usize) << 1
    }
}

/// One rasterised glyph image, placed relative to the top-left corner of its cell.
#[derive(Debug, Clone)]
pub struct GlyphImage {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// `true`: `data` is RGBA (colour emoji); `false`: `data` is an alpha mask.
    pub color: bool,
    pub data: Vec<u8>,
}

/// Cell size and the positions of lines inside a cell, in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellMetrics {
    pub width: u32,
    pub height: u32,
    /// Baseline, from the cell top.
    pub baseline: i32,
    /// Underline row, from the cell top.
    pub underline: i32,
    /// Strikethrough row, from the cell top.
    pub strike: i32,
    /// Thickness of underline, strikethrough and the bar / underline cursors.
    pub line: u32,
}

type Entry = [Option<Box<[GlyphImage]>>; 4];

/// The resolved font, its cell metrics and the glyph cache.
///
/// The cache is keyed by (grapheme cluster, bold/italic); it is emptied whenever size or scale
/// change, so every entry belongs to the current pixel size.
pub struct FontSet {
    system: FontSystem,
    swash: SwashCache,
    shaper: Buffer,
    config: FontConfig,
    /// The installed family name the configuration resolved to.
    family: String,
    font_px: f32,
    metrics: CellMetrics,
    ascii: Vec<Option<Box<[GlyphImage]>>>,
    clusters: HashMap<Box<str>, Entry>,
    cache_limit: usize,
}

impl std::fmt::Debug for FontSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontSet")
            .field("config", &self.config)
            .field("family", &self.family)
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

impl FontSet {
    /// Loads the system fonts (slow: up to a second; do it once) and applies `config`. A config
    /// that cannot be used gives the error back with a working one applied instead: a missing
    /// family keeps the asked size and line height (as [`FontSet::set_config`] callers do on a
    /// reload), anything else gives the default config.
    #[must_use]
    pub fn new(config: &FontConfig) -> (Self, Option<FontError>) {
        Self::with_font_system(system_font_system(), config)
    }

    /// As [`FontSet::new`] over a font system the caller built (tests, bundled fonts).
    #[must_use]
    pub fn with_font_system(system: FontSystem, config: &FontConfig) -> (Self, Option<FontError>) {
        let mut set = Self {
            shaper: Buffer::new_empty(Metrics::new(16.0, 16.0)),
            system,
            swash: SwashCache::new(),
            config: FontConfig::default(),
            family: String::new(),
            font_px: 16.0,
            metrics: CellMetrics {
                width: 1,
                height: 1,
                baseline: 1,
                underline: 1,
                strike: 1,
                line: 1,
            },
            ascii: Vec::new(),
            clusters: HashMap::new(),
            cache_limit: CACHE_LIMIT,
        };
        let fallback = FontConfig {
            scale: valid_scale(config.scale).unwrap_or(1.0),
            ..FontConfig::default()
        };
        match set.set_config(config) {
            Ok(()) => (set, None),
            Err(e @ FontError::FamilyNotFound(_)) => {
                let platform = FontConfig {
                    family: None,
                    ..config.clone()
                };
                if set.set_config(&platform).is_err() {
                    let family = set.resolve_family(None).unwrap_or_default();
                    set.apply(fallback, family);
                }
                (set, Some(e))
            }
            Err(e) => {
                let family = set.resolve_family(None).unwrap_or_default();
                set.apply(fallback, family);
                (set, Some(e))
            }
        }
    }

    /// Applies a new configuration (settings reload). On error nothing changes.
    pub fn set_config(&mut self, config: &FontConfig) -> Result<(), FontError> {
        if !(config.size_pt.is_finite() && config.size_pt > 0.0 && config.size_pt <= MAX_SIZE_PT) {
            return Err(FontError::InvalidSize(config.size_pt));
        }
        if !(config.line_height.is_finite() && (0.5..=4.0).contains(&config.line_height)) {
            return Err(FontError::InvalidLineHeight(config.line_height));
        }
        let scale = valid_scale(config.scale).ok_or(FontError::InvalidScale(config.scale))?;
        let family = self.resolve_family(config.family.as_deref())?;
        self.apply(
            FontConfig {
                scale,
                ..config.clone()
            },
            family,
        );
        Ok(())
    }

    /// Changes only the scale factor (the window moved to another screen).
    pub fn set_scale(&mut self, scale: f32) -> Result<(), FontError> {
        let config = FontConfig {
            scale,
            ..self.config.clone()
        };
        self.set_config(&config)
    }

    #[must_use]
    pub fn config(&self) -> &FontConfig {
        &self.config
    }

    /// The installed family the grid uses.
    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    #[must_use]
    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    /// Font size in device pixels.
    #[must_use]
    pub fn font_px(&self) -> f32 {
        self.font_px
    }

    /// Number of grapheme / style pairs rasterised so far.
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        let ascii = self.ascii.iter().filter(|e| e.is_some()).count();
        let other: usize = self
            .clusters
            .values()
            .map(|e| e.iter().filter(|s| s.is_some()).count())
            .sum();
        ascii + other
    }

    /// The images of one grapheme cluster, rasterised on first use.
    pub fn glyph(&mut self, cluster: &str, style: GlyphStyle) -> &[GlyphImage] {
        let cluster = &cluster[..cluster.floor_char_boundary(MAX_CLUSTER_BYTES)];
        let slot = style.slot();
        let bytes = cluster.as_bytes();
        if bytes.len() == 1 && bytes[0] < 128 {
            let index = usize::from(bytes[0]) * 4 + slot;
            if self.ascii[index].is_none() {
                self.ascii[index] = Some(self.rasterize(cluster, style));
            }
            return self.ascii[index].as_deref().unwrap_or_default();
        }
        let present = self
            .clusters
            .get(cluster)
            .is_some_and(|e| e[slot].is_some());
        if !present {
            let images = self.rasterize(cluster, style);
            if self.clusters.len() >= self.cache_limit && !self.clusters.contains_key(cluster) {
                self.clusters.clear();
            }
            self.clusters.entry(cluster.into()).or_default()[slot] = Some(images);
        }
        self.clusters
            .get(cluster)
            .and_then(|e| e[slot].as_deref())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn set_cache_limit(&mut self, limit: usize) {
        self.cache_limit = limit;
        self.clusters.clear();
    }

    fn resolve_family(&self, wanted: Option<&str>) -> Result<String, FontError> {
        let db = self.system.db();
        let installed = |name: &str| -> Option<String> {
            db.faces().find_map(|face| {
                face.families
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(n, _)| n.clone())
            })
        };
        if let Some(name) = wanted {
            return installed(name.trim()).ok_or_else(|| FontError::FamilyNotFound(name.into()));
        }
        let generic = db.family_name(&Family::Monospace).to_owned();
        let found = installed(&generic)
            .or_else(|| MONOSPACE_CANDIDATES.iter().find_map(|n| installed(n)))
            .or_else(|| {
                db.faces()
                    .find(|f| f.monospaced)
                    .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            });
        // No monospace face at all: shaping still falls back to whatever is installed.
        Ok(found.unwrap_or(generic))
    }

    fn apply(&mut self, config: FontConfig, family: String) {
        self.font_px = config.size_pt * PX_PER_PT * config.scale;
        let line_px = (self.font_px * config.line_height).round().max(1.0);
        self.config = config;
        self.family = family;
        self.shaper = Buffer::new(&mut self.system, Metrics::new(self.font_px, line_px));
        self.shaper.set_wrap(Wrap::None);
        self.ascii = vec![None; 128 * 4];
        self.clusters.clear();

        // Cell width from the advance of a full-width Latin letter, baseline from the font's
        // ascent centred in the line.
        let attrs = Attrs::new().family(Family::Name(&self.family));
        self.shaper.set_text("M", &attrs, Shaping::Advanced, None);
        self.shaper.shape_until_scroll(&mut self.system, false);
        let (advance, baseline) = self
            .shaper
            .layout_runs()
            .next()
            .map(|run| (run.glyphs.first().map_or(0.0, |g| g.w), run.line_y))
            .unwrap_or((0.0, self.font_px * 0.8));
        let advance = if advance > 0.0 {
            advance
        } else {
            self.font_px * 0.6
        };
        let height = line_px as u32;
        let line = (self.font_px / 14.0).round().max(1.0) as u32;
        let baseline = (baseline.round() as i32).clamp(1, height as i32);
        let below = (height as i32 - line as i32).max(0);
        self.metrics = CellMetrics {
            width: (advance.round() as u32).max(1),
            height,
            baseline,
            underline: (baseline + line as i32).min(below),
            strike: (baseline - (self.font_px * 0.3).round() as i32).clamp(0, below),
            line,
        };
    }

    fn rasterize(&mut self, cluster: &str, style: GlyphStyle) -> Box<[GlyphImage]> {
        let attrs = Attrs::new()
            .family(Family::Name(&self.family))
            .weight(if style.bold {
                Weight::BOLD
            } else {
                Weight::NORMAL
            })
            .style(if style.italic {
                Style::Italic
            } else {
                Style::Normal
            });
        self.shaper
            .set_text(cluster, &attrs, Shaping::Advanced, None);
        self.shaper.shape_until_scroll(&mut self.system, false);
        let glyphs: Vec<_> = self
            .shaper
            .layout_runs()
            .flat_map(|run| run.glyphs.iter().map(|g| g.physical((0.0, 0.0), 1.0)))
            .collect();
        let baseline = self.metrics.baseline;
        glyphs
            .into_iter()
            .filter_map(|glyph| {
                let image = self
                    .swash
                    .get_image_uncached(&mut self.system, glyph.cache_key)?;
                let color = match image.content {
                    SwashContent::Mask => false,
                    SwashContent::Color => true,
                    SwashContent::SubpixelMask => return None,
                };
                (image.placement.width > 0 && image.placement.height > 0).then(|| GlyphImage {
                    left: glyph.x + image.placement.left,
                    top: baseline + glyph.y - image.placement.top,
                    width: image.placement.width,
                    height: image.placement.height,
                    color,
                    data: image.data,
                })
            })
            .collect()
    }
}

fn valid_scale(scale: f32) -> Option<f32> {
    (scale.is_finite() && scale > 0.0 && scale <= 16.0).then_some(scale)
}

/// A font system over the installed fonts, keeping the platform's monospace family (cosmic-text's
/// own constructor replaces it with a fixed name).
#[must_use]
pub fn system_font_system() -> FontSystem {
    let db = system_database();
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned());
    FontSystem::new_with_locale_and_db(locale, db)
}

/// The installed fonts, with the generic `monospace` family set to what fontconfig would pick
/// where fontconfig is the platform's font configuration.
#[must_use]
pub(crate) fn system_database() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
    if let Some(family) = fontconfig_monospace(&db) {
        db.set_monospace_family(family);
    }
    db
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
use fontconfig_parser::Alias;

/// fontconfig's match for `monospace` among the installed families. fontdb takes the first
/// preferred name of the last `monospace` alias, installed or not (e.g. "FreeMono" from a
/// late conf.d file); fontconfig instead tries every alias in order.
#[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
fn fontconfig_monospace(db: &fontdb::Database) -> Option<String> {
    use std::path::{Path, PathBuf};

    // The files fontdb reads, in the same order.
    let mut config = fontconfig_parser::FontConfig::default();
    if let Some(file) = std::env::var_os("FONTCONFIG_FILE") {
        let _ = config.merge_config(Path::new(&file));
    } else {
        let xdg = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")));
        let user_read = xdg.is_some_and(|dir| {
            config
                .merge_config(&dir.join("fontconfig/fonts.conf"))
                .is_ok()
        });
        if !user_read {
            let _ = config.merge_config(Path::new("/etc/fonts/local.conf"));
        }
        let _ = config.merge_config(Path::new("/etc/fonts/fonts.conf"));
    }
    preferred_monospace(&config.aliases, |name| {
        db.faces()
            .any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
    })
}

/// The family fontconfig's alias rules put first for `monospace`, among installed ones: every
/// rule's `<prefer>` list is inserted before `monospace` in rule order, `<accept>` right after it
/// (so later rules come first), `<default>` at the end.
#[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
fn preferred_monospace(aliases: &[Alias], installed: impl Fn(&str) -> bool) -> Option<String> {
    let rules: Vec<&Alias> = aliases
        .iter()
        .filter(|a| a.alias.eq_ignore_ascii_case("monospace"))
        .collect();
    let prefer = rules.iter().flat_map(|a| a.prefer.iter());
    let accept = rules.iter().rev().flat_map(|a| a.accept.iter());
    let default = rules.iter().flat_map(|a| a.default.iter());
    prefer
        .chain(accept)
        .chain(default)
        .find(|name| installed(name))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> FontSet {
        let (set, err) = FontSet::new(&FontConfig::default());
        assert_eq!(err, None);
        set
    }

    #[test]
    fn the_glyph_cache_is_bounded() {
        let mut set = set();
        set.set_cache_limit(8);
        for c in 'α'..='ω' {
            set.glyph(&c.to_string(), GlyphStyle::default());
            assert!(
                set.cached_glyphs() <= 8 + 128 * 4,
                "{}",
                set.cached_glyphs()
            );
        }
        let non_ascii = set.cached_glyphs() - set.ascii.iter().filter(|e| e.is_some()).count();
        assert!(non_ascii <= 8, "{non_ascii}");
    }

    #[test]
    fn overlong_clusters_are_cut_before_shaping_and_caching() {
        let mut set = set();
        let long = format!("e{}", "\u{0301}".repeat(5000));
        let images = set.glyph(&long, GlyphStyle::default()).len();
        assert!(images <= MAX_CLUSTER_BYTES, "{images}");
        assert!(set.clusters.keys().all(|k| k.len() <= MAX_CLUSTER_BYTES));
    }

    #[test]
    fn font_sizes_above_the_cap_are_refused() {
        let mut set = set();
        let big = FontConfig {
            size_pt: 300.0,
            ..FontConfig::default()
        };
        assert_eq!(set.set_config(&big), Err(FontError::InvalidSize(300.0)));
    }

    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
    #[test]
    fn the_monospace_alias_takes_the_first_installed_preferred_family_in_config_order() {
        let alias = |name: &str, prefer: &[&str], default: &[&str]| Alias {
            alias: name.into(),
            prefer: prefer.iter().map(|s| (*s).into()).collect(),
            accept: Vec::new(),
            default: default.iter().map(|s| (*s).into()).collect(),
        };
        let aliases = [
            alias("Courier", &[], &["monospace"]),
            alias(
                "monospace",
                &["Missing Mono", "DejaVu Sans Mono", "Noto Mono"],
                &[],
            ),
            alias("monospace", &["FreeMono"], &[]),
            alias("monospace", &[], &["Last Resort Mono"]),
        ];
        let installed = |n: &str| n != "Missing Mono";
        assert_eq!(
            preferred_monospace(&aliases, installed).as_deref(),
            Some("DejaVu Sans Mono")
        );
        assert_eq!(
            preferred_monospace(&aliases[2..], |n| n == "Last Resort Mono").as_deref(),
            Some("Last Resort Mono")
        );
        assert_eq!(preferred_monospace(&aliases, |_| false), None);
    }

    /// The machine check from QC: `fc-match monospace` names "DejaVu Sans Mono" here. Runs only
    /// where that family is installed and fontconfig's config is the system one.
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
    #[test]
    fn the_default_family_is_the_fontconfig_monospace_match() {
        let set = set();
        let dejavu = set
            .system
            .db()
            .faces()
            .any(|f| f.families.iter().any(|(n, _)| n == "DejaVu Sans Mono"));
        let system_config = std::env::var_os("FONTCONFIG_FILE").is_none()
            && std::path::Path::new("/etc/fonts/conf.d/60-latin.conf").exists();
        if !(dejavu && system_config) {
            return;
        }
        assert_eq!(set.family(), "DejaVu Sans Mono");
    }
}
