//! The welcome screen (option B of T17) at 120x40, 80x24 and 40x12 with a fixed monitor
//! reading, its states (no Linux, missing fields, no files yet) and its colours.

use std::path::{Path, PathBuf};

use ratatui::style::Color;

use super::*;
use crate::monitor::{MemInfo, Monitor, Reading};
use crate::theme::{ColorDepth, contrast, to_rgb};

const ROOT: &str = "/home/user/DEV/shop";

/// 15.5 GiB RAM with 9.4 used, 2.0 GiB swap with 0.3 used.
fn reading() -> Reading {
    Reading {
        cpu: Some(37.0),
        cores: vec![42.0, 18.0, 63.0, 9.0, 88.0, 31.0, 22.0, 37.0],
        mem: MemInfo {
            total: Some(16_252_928),
            available: Some(6_396_314),
            swap_total: Some(2_097_152),
            swap_free: Some(1_782_579),
        },
        load: Some([1.42, 1.18, 0.97]),
        uptime: Some(3 * 86_400 + 4 * 3600 + 120),
        own_cpu: Some(1.2),
        own_rss: Some(49_152),
    }
}

fn history(offset: usize) -> Vec<f32> {
    (0..60)
        .map(|i| (((i + offset) * (i + offset) * 7 + (i + offset) * 13) % 60 + 10) as f32)
        .collect()
}

fn welcome_app() -> App {
    let (mut app, _) = test_app(Path::new(ROOT));
    app.keyboard_enhanced = true;
    app.monitor = Monitor::fixed(
        reading(),
        &history(0),
        &history(5).iter().map(|v| v / 20.0).collect::<Vec<_>>(),
    );
    app.monitor.branch = Some("main".into());
    let files = (0..1284)
        .map(|i| PathBuf::from(format!("src/f{i}.rs")))
        .collect();
    app.file_index = Some(vr_fs::FileIndex::from_paths(ROOT, files));
    app.recent = [
        "src/main.rs",
        "src/app.rs",
        "Cargo.toml",
        "README.md",
        "src/ui/editor_view.rs",
    ]
    .iter()
    .map(|p| Path::new(ROOT).join(p))
    .collect();
    app
}

fn lines(app: &mut App, w: u16, h: u16) -> Vec<String> {
    let t = draw(app, w, h);
    let text = format!("{}", t.backend());
    text.lines()
        .map(|l| l.trim_matches('"').to_string())
        .collect()
}

#[test]
fn welcome_snapshots() {
    let mut app = welcome_app();
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("welcome_{w}x{h}"), t.backend());
    }
}

#[test]
fn welcome_shows_the_dashboard_parts_at_120x40() {
    let mut app = welcome_app();
    let screen = lines(&mut app, 120, 40).join("\n");
    for needle in [
        "SYSTEM",
        "START",
        "RECENT",
        "VR-IDE",
        "CPU 37%",
        "58 s",
        "C0 ",
        "C7 ",
        "RAM ",
        "9.4/15.5 GiB",
        " 61%",
        "SWP ",
        "0.3/2.0 GiB",
        " 15%",
        "LOAD 1.42 1.18 0.97",
        "UP 3d 4h",
        "1.2%",
        "48 MiB",
        "0.3% of RAM",
        "main · 1,284 files",
        "src/main.rs",
        "ctrl+p",
        "Go to file",
        "f1",
        "Command palette",
        "ctrl+e",
        "Explorer",
        "Open file",
        "ctrl+q",
        "Quit",
    ] {
        assert!(screen.contains(needle), "{needle:?} missing:\n{screen}");
    }
    assert!(screen.contains("ctrl+o"), "ctrl+o missing:\n{screen}");
}

#[test]
fn welcome_has_no_welcome_key_and_no_tabs_row_content() {
    let mut app = welcome_app();
    let screen = lines(&mut app, 120, 40).join("\n");
    assert!(!screen.to_lowercase().contains("welcome"), "{screen}");
}

#[test]
fn compact_drops_everything_but_cpu_ram_and_four_keys() {
    let mut app = welcome_app();
    let screen = lines(&mut app, 40, 12).join("\n");
    for gone in ["SYSTEM", "SWP", "LOAD", "UP ", "VR-IDE", "RECENT", "│"] {
        assert!(
            !screen.contains(gone),
            "{gone:?} should be dropped:\n{screen}"
        );
    }
    for kept in [
        "CPU ",
        "RAM ",
        "Go to file",
        "Command palette",
        "Explorer",
        "Quit",
    ] {
        assert!(screen.contains(kept), "{kept:?} missing:\n{screen}");
    }
    // The row the project root used to take shows one more key (Open file).
    assert!(screen.contains("Open file"), "{screen}");
}

#[test]
fn every_row_stays_inside_the_screen_at_odd_sizes() {
    let mut app = welcome_app();
    for (w, h) in [
        (1, 1),
        (10, 3),
        (30, 8),
        (63, 15),
        (64, 16),
        (100, 20),
        (110, 26),
        (200, 60),
    ] {
        let t = draw(&mut app, w, h);
        assert_eq!(t.backend().buffer().area.width, w);
    }
}

#[test]
fn meters_use_the_three_thresholds() {
    let mut app = welcome_app();
    let theme = Theme::from_name(&app.settings.theme);
    let t = draw(&mut app, 120, 40);
    let buf = t.backend().buffer();
    let colour_of = |text: &str| {
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(i) = row.find(text) {
                let x = row[..i].chars().count() as u16;
                return buf[(x, y)].fg;
            }
        }
        panic!("{text} not drawn");
    };
    // 61 % RAM is orange (60..84), 15 % swap green, core 4 at 88 % red.
    assert_eq!(colour_of("61%"), theme.warning.fg.unwrap());
    assert_eq!(colour_of("15%"), theme.ok.fg.unwrap());
    assert_eq!(colour_of("88%"), theme.error.fg.unwrap());
    assert_eq!(colour_of("42%"), theme.ok.fg.unwrap());
}

#[test]
fn bars_show_the_true_share() {
    let mut app = welcome_app();
    let l = lines(&mut app, 80, 24);
    let ram = l.iter().find(|l| l.contains("RAM ")).unwrap();
    let filled = ram.matches('█').count();
    let track = ram.matches('░').count();
    // 14 cells at 60.6 % (rounded to 8).
    assert_eq!((filled, track), (8, 6), "{ram}");
}

#[test]
fn missing_fields_show_a_dash_and_never_panic() {
    let mut app = welcome_app();
    app.monitor = Monitor::fixed(Reading::default(), &[], &[]);
    app.monitor.branch = None;
    app.file_index = None;
    app.recent.clear();
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let screen = lines(&mut app, w, h).join("\n");
        assert!(screen.contains('—'), "{w}x{h}:\n{screen}");
    }
    let screen = lines(&mut app, 80, 24).join("\n");
    assert!(
        !screen.contains("RECENT"),
        "no recent block without files:\n{screen}"
    );
}

#[test]
fn off_linux_the_panel_says_so() {
    let mut app = welcome_app();
    app.monitor = Monitor::new(Box::new(crate::monitor::ProcFs), false);
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let screen = lines(&mut app, w, h).join("\n");
        assert!(screen.contains("monitor: Linux only"), "{w}x{h}:\n{screen}");
        assert!(!screen.contains("RAM "), "{w}x{h}:\n{screen}");
    }
}

#[test]
fn file_count_says_indexing_until_the_index_is_ready() {
    let mut app = welcome_app();
    app.file_index = None;
    let (_tx, rx) = std::sync::mpsc::channel();
    app.index_rx = Some(rx);
    let screen = lines(&mut app, 80, 24).join("\n");
    assert!(screen.contains("main · indexing…"), "{screen}");
}

#[test]
fn a_command_without_a_key_has_no_hint() {
    let mut app = welcome_app();
    // With `terminal.toggle` bound (M5) its row shows the key; with ctrl+` removed by the user
    // there is no "Terminal" row rather than "run terminal.toggle".
    let screen = lines(&mut app, 120, 40).join("\n");
    let row = screen
        .lines()
        .find(|l| l.contains("Terminal"))
        .expect("Terminal row");
    assert!(row.contains("ctrl+`"), "{screen}");
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("keymap.toml");
    std::fs::write(&user, "\"ctrl+`\" = \"\"\n[terminal]\n\"ctrl+`\" = \"\"\n").unwrap();
    app.keymap = vr_config::Keymap::load(Some(&user)).0;
    let screen = lines(&mut app, 120, 40).join("\n");
    assert!(
        !screen.contains("Terminal") && !screen.contains("run "),
        "{screen}"
    );
}

#[test]
fn recent_paths_are_relative_and_long_ones_are_cut_from_the_left() {
    let mut app = welcome_app();
    app.recent = vec![Path::new(ROOT).join("a/very/deeply/nested/directory/with/a/file_name.rs")];
    let screen = lines(&mut app, 80, 24).join("\n");
    let row = screen
        .lines()
        .find(|l| l.contains("file_name.rs"))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(
        row.contains("  …") && row.contains("/file_name.rs"),
        "{row}"
    );
}

/// Every drawn glyph that carries information reads on the theme background: 4.5:1 for text,
/// 3:1 for the separator tone. Tracks, rules and the divider are decorative and skipped.
#[test]
fn welcome_colours_meet_contrast_in_every_painted_theme() {
    for depth in [ColorDepth::TrueColor, ColorDepth::Indexed256] {
        for theme in [Theme::synthwave_with(depth), Theme::dracula_with(depth)] {
            let mut app = welcome_app();
            let bg = to_rgb(theme.text.bg.unwrap()).unwrap();
            for (w, h) in [(120, 40), (80, 24), (40, 12)] {
                let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
                t.draw(|f| render_with(f, &mut app, &theme)).unwrap();
                let buf = t.backend().buffer();
                for y in 1..h - 1 {
                    for x in 0..w {
                        let c = &buf[(x, y)];
                        let s = c.symbol();
                        if s.trim().is_empty() || matches!(s, "░" | "│" | "─") {
                            continue;
                        }
                        let (Color::Rgb(..) | Color::Indexed(_)) = c.fg else {
                            panic!("{s:?} at {x},{y} has no explicit colour");
                        };
                        let floor = if Some(c.fg) == theme.gutter.fg {
                            3.0
                        } else {
                            4.5
                        };
                        let ratio = contrast(to_rgb(c.fg).unwrap(), bg);
                        assert!(
                            ratio >= floor,
                            "{depth:?} {w}x{h} {s:?} at {x},{y}: {:?} on bg is {ratio:.2}",
                            c.fg
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn unpainted_themes_render_without_panicking() {
    for name in ["dark", "light"] {
        let mut app = welcome_app();
        app.settings.theme = name.into();
        for (w, h) in [(120, 40), (80, 24), (40, 12)] {
            let screen = lines(&mut app, w, h).join("\n");
            assert!(screen.contains("RAM "), "{name} {w}x{h}");
        }
    }
}

#[test]
fn compact_shows_every_hint_when_rows_allow() {
    let mut app = welcome_app();
    for (w, h) in [(50, 24), (60, 14)] {
        let screen = lines(&mut app, w, h).join("\n");
        for title in [
            "Go to file",
            "Command palette",
            "Explorer",
            "Open file",
            "Quit",
        ] {
            assert!(screen.contains(title), "{w}x{h} lacks {title:?}:\n{screen}");
        }
    }
}

#[test]
fn cut_cores_are_counted() {
    let mut app = welcome_app();
    let mut r = reading();
    r.cores = (0..24).map(|i| i as f32 * 4.0).collect();
    app.monitor = Monitor::fixed(r, &history(0), &history(5));
    // 120x40 shows 12 core bars, 80x24 as many glyphs as fit before the marker.
    let full = lines(&mut app, 120, 40).join("\n");
    assert!(full.contains("C11 ") && !full.contains("C12 "), "{full}");
    assert!(full.contains("+12"), "{full}");
    let medium = lines(&mut app, 80, 24);
    let row = medium.iter().find(|l| l.contains("CORE")).unwrap();
    let shown = row.matches(|c: char| "▁▂▃▄▅▆▇█".contains(c)).count();
    assert!(
        shown < 24 && row.contains(&format!("+{}", 24 - shown)),
        "{row}"
    );
    // All cores fit: no marker.
    app.monitor = Monitor::fixed(reading(), &history(0), &history(5));
    let medium = lines(&mut app, 80, 24);
    assert!(!medium.iter().any(|l| l.contains("CORE") && l.contains('+')));
}

#[test]
fn history_label_matches_the_graph_width() {
    let mut app = welcome_app();
    let screen = lines(&mut app, 120, 40).join("\n");
    assert!(
        screen.contains("58 s") && !screen.contains("60 s"),
        "{screen}"
    );
    // Wider than the ring: the label is the ring length.
    let screen = lines(&mut app, 160, 40).join("\n");
    assert!(screen.contains("60 s"), "{screen}");
}

#[test]
fn legacy_keyboard_welcome_shows_the_same_reachable_keys() {
    let mut app = welcome_app();
    app.keyboard_enhanced = false;
    let screen = lines(&mut app, 120, 40).join("\n");
    // Every default key is deliverable by a legacy terminal (USER-5): nothing is left out.
    let open = screen
        .lines()
        .find(|l| l.contains("Open file"))
        .expect("Open file hint");
    assert!(open.contains("ctrl+o"), "{open}");
    for kept in ["ctrl+p", "f1", "ctrl+e", "ctrl+q", "ctrl+`"] {
        assert!(screen.contains(kept), "{kept}:\n{screen}");
    }
    for gone in ["alt+", "ctrl+shift", "ctrl+alt"] {
        assert!(!screen.contains(gone), "{gone}:\n{screen}");
    }
    let t = draw(&mut app, 120, 40);
    insta::assert_snapshot!("welcome_legacy_keyboard_120x40", t.backend());
}

/// The compact welcome leaves the project root to the status bar and gives its row to a hint.
#[test]
fn compact_welcome_does_not_repeat_the_root_and_shows_one_more_hint() {
    let mut app = welcome_app();
    let l = lines(&mut app, 40, 10);
    // Row 0 is the tab row.
    let body = l[1..l.len() - 1].join("\n");
    assert!(!body.contains("/home/user"), "{body}");
    assert!(l.last().unwrap().contains("/home/user"), "{l:?}");
    let shown = [
        "Go to file",
        "Command palette",
        "Explorer",
        "Quit",
        "Open file",
        "Terminal",
    ]
    .iter()
    .filter(|t| body.contains(*t))
    .count();
    assert_eq!(shown, 3, "{body}");
}

// ---- several roots (W-2) ----------------------------------------------------------------------

const WEB: &str = "/home/user/DEV/web";
const DOCS: &str = "/home/user/DEV/docs";

/// `welcome_app` with three roots (`docs` gone from disk), their branches, an index over
/// them and two recent workspaces.
fn multi_root_app() -> App {
    use crate::app::RootInfo;
    let mut app = welcome_app();
    let roots = [PathBuf::from(ROOT), PathBuf::from(WEB), PathBuf::from(DOCS)];
    let (ws, _) = crate::workspace::Workspace::from_paths(&roots).unwrap();
    app.workspace = ws;
    app.root_info = vec![
        RootInfo {
            path: PathBuf::from(ROOT),
            branch: Some("main".into()),
            missing: false,
        },
        RootInfo {
            path: PathBuf::from(WEB),
            branch: Some("feature/login".into()),
            missing: false,
        },
        RootInfo {
            path: PathBuf::from(DOCS),
            branch: None,
            missing: true,
        },
    ];
    let mut files: Vec<PathBuf> = (0..1284)
        .map(|i| PathBuf::from(format!("shop/src/f{i}.rs")))
        .collect();
    files.extend((0..312).map(|i| PathBuf::from(format!("web/src/c{i}.ts"))));
    app.file_index = Some(vr_fs::FileIndex::from_paths(ROOT, files));
    app.count_root_files();
    app.recent_workspaces.entries = vec![
        vr_config::RecentWorkspace {
            file: None,
            roots: roots.to_vec(),
        },
        vr_config::RecentWorkspace {
            file: Some(PathBuf::from("/home/user/DEV/team.vr-workspace")),
            roots: vec![PathBuf::from(ROOT), PathBuf::from("/home/user/DEV/api")],
        },
        vr_config::RecentWorkspace {
            file: None,
            roots: vec![PathBuf::from("/home/user/old/blog")],
        },
    ];
    app
}

#[test]
fn welcome_multi_root_snapshots() {
    let mut app = multi_root_app();
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("welcome_multi_root_{w}x{h}"), t.backend());
    }
}

#[test]
fn welcome_lists_each_root_and_the_recent_workspaces() {
    let mut app = multi_root_app();
    let screen = lines(&mut app, 120, 40).join("\n");
    for needle in [
        "PROJECT",
        "shop",
        "main · 1,284 files",
        "web",
        "feature/login · 312 files",
        "docs",
        "missing",
        "WORKSPACES",
        "team",
        "blog",
    ] {
        assert!(screen.contains(needle), "{needle:?} missing:\n{screen}");
    }
    // Numbered as `/ws recent` takes them; the workspace now open (1) is not offered again.
    assert!(
        screen.contains("2  team") && screen.contains("3  blog"),
        "{screen}"
    );
    assert!(!screen.contains("1  shop"), "{screen}");
    // One root: no PROJECT list, the dashboard as before (snapshots above).
    let mut one = welcome_app();
    let screen = lines(&mut one, 120, 40).join("\n");
    assert!(!screen.contains("PROJECT"), "{screen}");
}

#[test]
fn a_click_on_a_recent_workspace_opens_it() {
    let d = tempfile::tempdir().unwrap();
    let base = d.path().canonicalize().unwrap();
    for f in ["a", "b", "c"] {
        std::fs::create_dir(base.join(f)).unwrap();
    }
    let (mut app, _) = test_app(&base.join("a"));
    app.recent_workspaces.entries = vec![
        vr_config::RecentWorkspace {
            file: None,
            roots: vec![base.join("a")],
        },
        vr_config::RecentWorkspace {
            file: None,
            roots: vec![base.join("b"), base.join("c")],
        },
    ];
    let t = draw(&mut app, 120, 40);
    let text = format!("{}", t.backend());
    let (y, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("b, c"))
        .unwrap_or_else(|| panic!("no recent workspace row:\n{text}"));
    let shown = line.trim_matches('"');
    let x = shown[..shown.find("b, c").unwrap()].chars().count() as u16;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: x,
        row: y as u16,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.workspace.paths(), [base.join("b"), base.join("c")]);
}
