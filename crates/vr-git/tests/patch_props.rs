//! AC-16f / SEC-10 property test: random files (CRLF, missing final newline, unicode, empty,
//! tab-indented), random edits, random hunk and line selections. Staging, unstaging and
//! discarding must give exactly what applying the same selection in memory gives; selections
//! git cannot express are refused with the index (or file) unchanged. Whole-hunk staging is
//! also compared with `git add -p`.

mod common;

use std::path::Path;

use common::Fixture;
use vr_git::{FileDiff, GitError, LineKind};

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
}

const VOCAB: &[&str] = &[
    "alpha",
    "beta\t\tgamma",
    "\tindented",
    "",
    "héllo ✓ wörld",
    "x = 1;",
    "crlf line\r",
    "    four spaces",
    "end",
    "日本語",
];

type Lines = Vec<(Vec<u8>, bool)>;

fn split(b: &[u8]) -> Lines {
    let mut v: Lines = Vec::new();
    let mut rest = b;
    while !rest.is_empty() {
        match rest.iter().position(|&c| c == b'\n') {
            Some(i) => {
                v.push((rest[..i].to_vec(), true));
                rest = &rest[i + 1..];
            }
            None => {
                v.push((rest.to_vec(), false));
                rest = &[];
            }
        }
    }
    v
}

/// `None` when a line without newline would not be the last one (git cannot apply that).
fn join(lines: &Lines) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (i, (t, eol)) in lines.iter().enumerate() {
        if !eol && i + 1 != lines.len() {
            return None;
        }
        out.extend_from_slice(t);
        if *eol {
            out.push(b'\n');
        }
    }
    Some(out)
}

/// Short files (0-11 lines) or long ones (30-200 lines) whose edits are spread far apart, so
/// diffs with several hunks (and a hunk other than the first) are common.
fn random_file(rng: &mut Rng, long: bool) -> Vec<u8> {
    let n = if long {
        30 + rng.below(171)
    } else {
        rng.below(12)
    };
    let lines: Lines = (0..n)
        .map(|_| (VOCAB[rng.below(VOCAB.len())].as_bytes().to_vec(), true))
        .collect();
    let mut b = join(&lines).unwrap();
    if !b.is_empty() && rng.chance(25) {
        b.pop();
    }
    b
}

fn random_edit(rng: &mut Rng, base: &[u8], long: bool) -> Vec<u8> {
    let mut lines = split(base);
    let edits = if long {
        2 + rng.below(7)
    } else {
        1 + rng.below(4)
    };
    for _ in 0..edits {
        let pick = |rng: &mut Rng| {
            if rng.chance(50) {
                VOCAB[rng.below(VOCAB.len())].as_bytes().to_vec()
            } else {
                format!("changed {}", rng.below(1000)).into_bytes()
            }
        };
        match rng.below(3) {
            0 if !lines.is_empty() => {
                let i = rng.below(lines.len());
                lines[i].0 = pick(rng);
            }
            1 if !lines.is_empty() => {
                lines.remove(rng.below(lines.len()));
            }
            _ => {
                let i = rng.below(lines.len() + 1);
                lines.insert(i, (pick(rng), true));
            }
        }
    }
    for l in lines.iter_mut() {
        l.1 = true;
    }
    let mut b = join(&lines).unwrap();
    if !b.is_empty() && rng.chance(25) {
        b.pop();
    }
    b
}

fn random_selection(rng: &mut Rng, d: &FileDiff, h: usize) -> Vec<usize> {
    let lines = &d.hunks[h].lines;
    let whole = rng.chance(25);
    (0..lines.len())
        .filter(|&i| lines[i].kind != LineKind::Context && (whole || rng.chance(50)))
        .collect()
}

/// Apply the selection of hunk `h` in memory. Forward: the old side (index) gets the selected
/// changes. Reverse: the new side loses them.
fn model(target: &[u8], d: &FileDiff, h: usize, sel: &[usize], reverse: bool) -> Option<Vec<u8>> {
    let hunk = &d.hunks[h];
    let mut out: Lines = Vec::new();
    for (i, l) in hunk.lines.iter().enumerate() {
        let s = sel.contains(&i);
        let keep = match (l.kind, reverse) {
            (LineKind::Context, _) => true,
            (LineKind::Removed, false) => !s,
            (LineKind::Added, false) => s,
            (LineKind::Added, true) => !s,
            (LineKind::Removed, true) => s,
        };
        if keep {
            out.push((l.text.clone(), !l.no_newline));
        }
    }
    let (start, len) = if reverse {
        (hunk.new_start, hunk.new_len)
    } else {
        (hunk.old_start, hunk.old_len)
    };
    let from = if len == 0 { start } else { start - 1 } as usize;
    let mut lines = split(target);
    lines.splice(from..from + len as usize, out);
    join(&lines)
}

fn check_outcome(
    r: Result<(), GitError>,
    expected: Option<Vec<u8>>,
    actual: Vec<u8>,
    before: &[u8],
    what: &str,
) {
    match expected {
        Some(want) => {
            assert!(r.is_ok(), "{what}: {r:?}");
            assert_eq!(
                String::from_utf8_lossy(&actual),
                String::from_utf8_lossy(&want),
                "{what}"
            );
        }
        None => {
            assert!(matches!(r, Err(GitError::Refused(_))), "{what}: {r:?}");
            assert_eq!(actual, before, "{what}: refused but changed");
        }
    }
}

const CASES: u64 = 150;

#[test]
fn staging_lines_equals_the_in_memory_model() {
    let mut refused = 0;
    let mut later = 0;
    for seed in 1..=CASES {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let long = seed % 3 == 0;
        let base = random_file(&mut rng, long);
        let edit = random_edit(&mut rng, &base, long);
        let f = Fixture::new();
        f.write("f.txt", &base);
        f.commit_all("base");
        f.write("f.txt", &edit);
        let Some(d) = f.repo().diff_unstaged(Path::new("f.txt")).unwrap() else {
            continue;
        };
        let h = rng.below(d.hunks.len());
        later += usize::from(h > 0);
        let sel = random_selection(&mut rng, &d, h);
        if sel.is_empty() {
            continue;
        }
        let expected = model(&base, &d, h, &sel, false);
        refused += usize::from(expected.is_none());
        let r = f.writer().stage_lines(&d, h, &sel);
        let what = format!("seed {seed} base {base:?} edit {edit:?} hunk {h} sel {sel:?}");
        check_outcome(r, expected, f.index_bytes("f.txt"), &base, &what);
    }
    assert!(
        later >= 15,
        "only {later} cases chose a hunk after the first"
    );
    eprintln!("stage: {refused} refused selections");
}

#[test]
fn unstaging_lines_equals_the_in_memory_model() {
    let mut later = 0;
    for seed in 1..=CASES {
        let mut rng = Rng(seed.wrapping_mul(0xd1b5_4a32_d192_ed03));
        let long = seed % 3 == 0;
        let base = random_file(&mut rng, long);
        let edit = random_edit(&mut rng, &base, long);
        let f = Fixture::new();
        f.write("f.txt", &base);
        f.commit_all("base");
        f.write("f.txt", &edit);
        f.git(&["add", "f.txt"]);
        let Some(d) = f.repo().diff_staged(Path::new("f.txt")).unwrap() else {
            continue;
        };
        let h = rng.below(d.hunks.len());
        later += usize::from(h > 0);
        let sel = random_selection(&mut rng, &d, h);
        if sel.is_empty() {
            continue;
        }
        let expected = model(&edit, &d, h, &sel, true);
        let r = f.writer().unstage_lines(&d, h, &sel);
        let what = format!("seed {seed} base {base:?} edit {edit:?} hunk {h} sel {sel:?}");
        check_outcome(r, expected, f.index_bytes("f.txt"), &edit, &what);
        // The worktree is never touched by an unstage.
        assert_eq!(f.read("f.txt"), edit, "{what}");
    }
    assert!(
        later >= 15,
        "only {later} cases chose a hunk after the first"
    );
}

#[test]
fn discarding_lines_equals_the_in_memory_model() {
    let mut later = 0;
    for seed in 1..=CASES {
        let mut rng = Rng(seed.wrapping_mul(0x94d0_49bb_1331_11eb));
        let long = seed % 3 == 0;
        let base = random_file(&mut rng, long);
        let edit = random_edit(&mut rng, &base, long);
        let f = Fixture::new();
        f.write("f.txt", &base);
        f.commit_all("base");
        f.write("f.txt", &edit);
        let Some(d) = f.repo().diff_unstaged(Path::new("f.txt")).unwrap() else {
            continue;
        };
        let h = rng.below(d.hunks.len());
        later += usize::from(h > 0);
        let sel = random_selection(&mut rng, &d, h);
        if sel.is_empty() {
            continue;
        }
        let expected = model(&edit, &d, h, &sel, true);
        let w = f.writer();
        let r = w
            .prepare_discard_hunk(&d, h, Some(&sel))
            .and_then(|plan| w.discard(&plan, false))
            .map(|done| {
                assert_eq!(std::fs::read(done.recovery.unwrap()).unwrap(), edit);
            });
        let what = format!("seed {seed} base {base:?} edit {edit:?} hunk {h} sel {sel:?}");
        check_outcome(r, expected, f.read("f.txt"), &edit, &what);
        assert_eq!(f.index_bytes("f.txt"), base, "{what}: index touched");
    }
    assert!(
        later >= 15,
        "only {later} cases chose a hunk after the first"
    );
}

#[test]
fn staging_whole_hunks_equals_git_add_p() {
    for seed in 1..=CASES {
        let mut rng = Rng(seed.wrapping_mul(0xbf58_476d_1ce4_e5b9));
        let long = seed % 3 == 0;
        let base = random_file(&mut rng, long);
        let edit = random_edit(&mut rng, &base, long);
        let f = Fixture::new();
        f.write("f.txt", &base);
        f.commit_all("base");
        f.write("f.txt", &edit);
        let r = f.repo();
        let Some(d) = r.diff_unstaged(Path::new("f.txt")).unwrap() else {
            continue;
        };
        let picks: Vec<bool> = d.hunks.iter().map(|_| rng.chance(50)).collect();
        let w = f.writer();
        // Highest first: the hunks before a staged one keep their index.
        for h in (0..picks.len()).rev().filter(|&h| picks[h]) {
            let fresh = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
            w.stage_hunk(&fresh, h).unwrap();
        }
        let ours = f.index_bytes("f.txt");
        f.git(&["read-tree", "HEAD"]);
        let answers: String = picks
            .iter()
            .map(|&p| if p { "y\n" } else { "n\n" })
            .collect();
        let (code, _, err) = f.git_raw(&["add", "-p", "--", "f.txt"], Some(answers.as_bytes()));
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&f.index_bytes("f.txt")),
            "seed {seed} base {base:?} edit {edit:?} picks {picks:?}"
        );
    }
}
