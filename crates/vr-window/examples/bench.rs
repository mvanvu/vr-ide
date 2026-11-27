//! Renderer timing: full redraw (every cell changed) and a one-line change, at 1920x1080 and
//! 3840x2160, scale 1 and 2. Run in release:
//! `cargo run -p vr-window --example bench --release [-- --dump <file.ppm>]`.
//! `--dump` writes the 1920x1080 scale-1 frame as a PPM image for a visual check.

use std::time::{Duration, Instant};

use cosmic_text::{FontSystem, fontdb};
use ratatui::backend::Backend;
use ratatui::buffer::Cell;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;
use vr_window::{FontConfig, FontSet, Palette, PixelBackend};

const RUNS: usize = 41;
const TEXT: [&str; 4] = [
    "fn main() { let việt = \"Tiếng Việt có dấu: ệ ở ữ ặ\"; println!(\"{việt}\"); } ",
    "中文字符 日本語のテキスト 한국어 ",
    "// Mixed: ASCII + Vietnamese (Nguyễn Thị Hương) + CJK 漢字 + symbols →✓★ ",
    "struct Grid { cols: u16, rows: u16 } impl Grid { fn new() -> Self { todo!() } } ",
];

/// A full screen of cells; `phase` changes every symbol and colour.
fn frame(cols: u16, rows: u16, phase: usize) -> Vec<(u16, u16, Cell)> {
    let mut out = Vec::with_capacity(cols as usize * rows as usize);
    for row in 0..rows {
        let text = TEXT[(row as usize + phase) % TEXT.len()];
        let mut graphemes = unicode_segmentation::UnicodeSegmentation::graphemes(text, true)
            .cycle()
            .skip(phase * 3 + row as usize);
        let mut col = 0;
        while col < cols {
            let g = graphemes.next().unwrap_or(" ");
            let width = g.width().clamp(1, 2) as u16;
            if col + width > cols {
                out.push((col, row, Cell::EMPTY));
                break;
            }
            let mut cell = Cell::EMPTY;
            let hue = (usize::from(col) * 7 + usize::from(row) * 13 + phase * 50) as u8;
            cell.set_symbol(g)
                .set_fg(Color::Rgb(200, hue, 255 - hue))
                .set_bg(if (row as usize + phase).is_multiple_of(5) {
                    Color::Indexed(236)
                } else {
                    Color::Rgb(30, 30, hue / 8)
                });
            if col.is_multiple_of(11) {
                cell.modifier = Modifier::BOLD;
            }
            out.push((col, row, cell));
            col += width;
        }
    }
    out
}

fn median(mut v: Vec<Duration>) -> f64 {
    v.sort();
    v[v.len() / 2].as_secs_f64() * 1000.0
}

fn main() {
    let dump = std::env::args().skip_while(|a| a != "--dump").nth(1);
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    println!("size        scale  grid      cold-full-ms  full-ms(median)  line-ms(median)");
    for (w, h) in [(1920u32, 1080u32), (3840, 2160)] {
        for scale in [1.0f32, 2.0] {
            let system = FontSystem::new_with_locale_and_db("en-US".into(), db.clone());
            let config = FontConfig {
                scale,
                ..FontConfig::default()
            };
            let (fonts, err) = FontSet::with_font_system(system, &config);
            assert!(err.is_none(), "{err:?}");
            let mut b = PixelBackend::new(fonts, Palette::default(), w, h);
            let size = b.size().expect("size");
            let frames = [
                frame(size.width, size.height, 0),
                frame(size.width, size.height, 1),
            ];

            let start = Instant::now();
            b.draw(frames[0].iter().map(|(x, y, c)| (*x, *y, c)))
                .expect("draw");
            b.flush().expect("flush");
            let cold = start.elapsed().as_secs_f64() * 1000.0;
            // Warm both frames' glyphs, then time alternating full redraws.
            b.draw(frames[1].iter().map(|(x, y, c)| (*x, *y, c)))
                .expect("draw");
            let mut full = Vec::with_capacity(RUNS);
            for i in 0..RUNS {
                let f = &frames[i % 2];
                let start = Instant::now();
                b.draw(f.iter().map(|(x, y, c)| (*x, *y, c))).expect("draw");
                b.flush().expect("flush");
                let _ = b.take_damage();
                full.push(start.elapsed());
            }
            if let (Some(path), 1920, 1.0) = (&dump, w, scale) {
                write_ppm(path, b.canvas());
            }
            let row = size.height / 2;
            let mut line = Vec::with_capacity(RUNS);
            for i in 0..RUNS {
                let f = &frames[i % 2];
                let start = Instant::now();
                b.draw(
                    f.iter()
                        .filter(|(_, y, _)| *y == row)
                        .map(|(x, y, c)| (*x, *y, c)),
                )
                .expect("draw");
                b.flush().expect("flush");
                let _ = b.take_damage();
                line.push(start.elapsed());
            }
            println!(
                "{w}x{h}  {scale:<5}  {:>3}x{:<3}   {cold:>10.2}  {:>15.2}  {:>15.3}",
                size.width,
                size.height,
                median(full),
                median(line)
            );
        }
    }
}

fn write_ppm(path: &str, canvas: &vr_window::Canvas) {
    let mut out = format!("P6\n{} {}\n255\n", canvas.width(), canvas.height()).into_bytes();
    for p in canvas.pixels() {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(path, out).expect("write dump");
}
