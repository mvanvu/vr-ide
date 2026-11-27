//! AC-P1: the palette shows the selected row's full description under the list.

use super::*;
use crate::commands::palette::PaletteItem;

fn item(title: &str, detail: &str) -> PaletteItem {
    PaletteItem {
        name: "",
        title: title.into(),
        path: None,
        detail: detail.into(),
        keys: String::new(),
        hits: Vec::new(),
    }
}

fn screen(t: &Terminal<TestBackend>, w: u16, h: u16) -> Vec<String> {
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

const LONG: &str = "Shows the long description in full, ending TAILWORD";

fn palette_with(items: Vec<PaletteItem>, input: &str) -> App {
    let (_d, _p, mut app, _) = app_with_file("fn main() {}\n");
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    for c in input.chars() {
        key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    let p = app.palette.as_mut().unwrap();
    p.items = items;
    p.selected = 0;
    p.top = 0;
    app
}

#[test]
fn full_description_is_readable_at_three_sizes() {
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let mut app = palette_with(vec![item("/long", LONG), item("/b", "short")], "/");
        let t = draw(&mut app, w, h);
        let rows = screen(&t, w, h);
        let all = rows.join("\n");
        assert!(all.contains("TAILWORD"), "{w}x{h}:\n{all}");
        assert!(rows.iter().any(|r| r.contains("> /")), "input {w}x{h}");
        assert!(rows.iter().any(|r| r.contains("/long")), "row {w}x{h}");
    }
}

#[test]
fn detail_is_cut_to_three_lines_and_follows_selection() {
    let huge = "word ".repeat(200);
    let (w, h) = (40, 24);
    let mut app = palette_with(vec![item("/a", &huge), item("/b", LONG)], "/");
    let all = screen(&draw(&mut app, w, h), w, h).join("\n");
    assert!(all.contains('…'), "{all}");
    assert!(!all.contains("TAILWORD"), "{all}");
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    let all = screen(&draw(&mut app, w, h), w, h).join("\n");
    assert!(all.contains("TAILWORD"), "{all}");
    // A click selects the row too; the area follows.
    let mut app = palette_with(
        vec![
            item("/a", LONG),
            item(
                "/b",
                "other TAILB text that is long enough to be cut at the row end for sure",
            ),
        ],
        "/",
    );
    app.palette.as_mut().unwrap().selected = 1;
    let all = screen(&draw(&mut app, 40, 24), 40, 24).join("\n");
    assert!(all.contains("TAILB"), "{all}");
    assert!(!all.contains("TAILWORD"), "{all}");
}

fn file_item(title: &str, folder: &str) -> PaletteItem {
    PaletteItem {
        path: Some(std::path::PathBuf::from(title)),
        ..item(title, folder)
    }
}

fn cmd_item(title: &str, id: &'static str, keys: &str) -> PaletteItem {
    PaletteItem {
        name: id,
        keys: keys.into(),
        ..item(title, id)
    }
}

/// Rows between the list and the bottom border (separator plus detail lines).
fn below_list(app: &App) -> u16 {
    let p = app.palette.as_ref().unwrap();
    (p.area.y + p.area.height - 1) - (p.list_area.y + p.list_area.height)
}

#[test]
fn a_cut_file_name_is_readable_in_full() {
    let long = "a_really_long_file_name_that_will_not_fit_ENDMARK.rs";
    for items in [
        vec![file_item(long, ""), file_item("b.rs", "")],
        vec![file_item(long, "some/folder"), file_item("b.rs", "x")],
    ] {
        let mut app = palette_with(items, "");
        app.palette.as_mut().unwrap().mode = crate::commands::palette::PaletteMode::Files;
        let all = screen(&draw(&mut app, 40, 12), 40, 12).join("\n");
        assert!(all.contains("ENDMARK"), "{all}");
    }
}

#[test]
fn a_cut_slash_title_is_readable_in_full() {
    let title = "/ws remove <a very long workspace folder name> ENDMARK";
    let mut app = palette_with(vec![item(title, "Remove it")], "/");
    let all = screen(&draw(&mut app, 40, 12), 40, 12).join("\n");
    assert!(all.contains("ENDMARK"), "{all}");
}

#[test]
fn a_command_id_is_never_the_description() {
    let mut app = palette_with(
        vec![cmd_item("Save (overwrite)", "file.save_overwrite_ID", "")],
        "",
    );
    let all = screen(&draw(&mut app, 40, 12), 40, 12).join("\n");
    assert!(!all.contains("save_overwrite_ID"), "{all}");
    assert_eq!(below_list(&app), 0, "no area for a fitting title");
}

#[test]
fn a_command_with_cut_keys_shows_title_and_keys() {
    let mut app = palette_with(
        vec![cmd_item(
            "A command with a rather long title",
            "x.y",
            "ctrl+shift+q",
        )],
        "",
    );
    let all = screen(&draw(&mut app, 40, 12), 40, 12).join("\n");
    assert!(all.contains("ctrl+shift+q"), "{all}");
}

#[test]
fn arrowing_never_resizes_the_box() {
    let long = "a_really_long_file_name_that_will_not_fit_ENDMARK.rs";
    let mut items: Vec<PaletteItem> = (0..20)
        .map(|i| file_item(&format!("f{i}.rs"), "d"))
        .collect();
    items[7] = file_item(long, "");
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let mut app = palette_with(items.clone(), "");
        app.palette.as_mut().unwrap().mode = crate::commands::palette::PaletteMode::Files;
        draw(&mut app, w, h);
        let first = {
            let p = app.palette.as_ref().unwrap();
            (p.area, p.list_area)
        };
        for _ in 0..19 {
            key(&mut app, KeyCode::Down, KeyModifiers::NONE);
            let t = draw(&mut app, w, h);
            let p = app.palette.as_ref().unwrap();
            assert_eq!((p.area, p.list_area), first, "{w}x{h} sel {}", p.selected);
            assert!(p.list_area.height >= 1);
            assert!(p.selected >= p.top && p.selected < p.top + usize::from(p.list_area.height));
            let rows = screen(&t, w, h);
            assert!(rows.iter().any(|r| r.contains("> ")), "input {w}x{h}");
            assert!(p.area.y + p.area.height < h, "status row {w}x{h}");
        }
    }
}

#[test]
fn an_error_line_and_the_area_share_the_box() {
    let mut app = palette_with(vec![item("/long", LONG)], "/");
    app.palette.as_mut().unwrap().error = Some("unknown thing".into());
    let all = screen(&draw(&mut app, 40, 12), 40, 12).join("\n");
    assert!(
        all.contains("unknown thing") && all.contains("TAILWORD"),
        "{all}"
    );
    assert!(all.contains("> /"), "{all}");
}

#[test]
fn no_area_rows_when_every_row_fits() {
    let mut app = palette_with(vec![item("/a", "short"), item("/b", "tiny")], "/");
    draw(&mut app, 80, 24);
    assert_eq!(below_list(&app), 0);
}

#[test]
fn the_area_reserves_only_the_lines_a_cut_row_needs() {
    // A title two cells too wide for the 32-cell row wraps to two lines: separator + 2.
    let barely = "x".repeat(20) + " " + &"y".repeat(13);
    let mut app = palette_with(vec![cmd_item(&barely, "a.b", ""), item("/z", "")], "");
    draw(&mut app, 40, 12);
    assert_eq!(below_list(&app), 3);
    // The same list with a row that needs the three lines reserves them all.
    let mut app = palette_with(
        vec![
            cmd_item(&barely, "a.b", ""),
            cmd_item(&"w ".repeat(40), "c.d", ""),
        ],
        "",
    );
    draw(&mut app, 40, 12);
    assert_eq!(below_list(&app), 4);
}
