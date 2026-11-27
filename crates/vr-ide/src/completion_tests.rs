//! D6 / AC-7: Tab completion in the palette input (02_ui-spec.md S5), driven through the `App`
//! API with real folders under /tmp.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::commands::completion::{self, Phase, SshSlot};
use crate::slash_tests::{input, key, slash, titles, typ};
use crate::ssh::recent::Entry;

fn back_tab(app: &mut App) {
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
}

fn phase(app: &App) -> Option<Phase> {
    app.palette
        .as_ref()
        .and_then(|p| p.completion.as_ref())
        .map(|c| c.phase)
}

/// `/tmp/vrT4……` (fixed length, so drawn paths are stable) with `DEV/` holding the folders of
/// the mockups; the app's root is `DEV/<root>` and `DEV/<root>/session.rs` is the active file.
struct Fx {
    dir: tempfile::TempDir,
    app: App,
}

impl Fx {
    fn new(root: &str) -> Fx {
        let dir = tempfile::Builder::new()
            .prefix("vrT4")
            .tempdir_in("/tmp")
            .unwrap();
        let dev = dir.path().join("DEV");
        for f in [
            "api-gateway",
            "dotfiles",
            "fixecom",
            "fixecom-admin",
            "notes",
            "shop",
            "vr-ide",
            ".hidden",
        ] {
            std::fs::create_dir_all(dev.join(f)).unwrap();
        }
        std::fs::write(dev.join("todo.txt"), "").unwrap();
        std::fs::write(dev.join("team.vr-workspace"), "").unwrap();
        let creds = dev.join("fixecom/credentials");
        std::fs::create_dir_all(&creds).unwrap();
        for f in ["id_fixecom", "id_fixecom.pub", "id_fixecom_deploy"] {
            std::fs::write(creds.join(f), "").unwrap();
        }
        let project = dev.join(root);
        std::fs::write(project.join("session.rs"), "pub struct Session;\n").unwrap();
        std::fs::write(project.join("README.md"), "").unwrap();
        let (mut app, _) = test_app(&project);
        app.open_path(&project.join("session.rs")).unwrap();
        // The temp folder is the home: the base folder shows as `~/DEV/…` (S5).
        let home = dir.path().to_path_buf();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(
            home.join(".ssh/config"),
            "Host app.fixecom.vn\nHost app-staging\nHost bastion\nHost git.example.com\n",
        )
        .unwrap();
        app.ssh.home = Some(home);
        app.ssh.recent =
            vec![Entry::new("-i credentials/id_fixecom root@app.fixecom.vn", None).unwrap()];
        Fx { dir, app }
    }

    fn dev(&self) -> PathBuf {
        self.dir.path().join("DEV")
    }
}

// ---- command names --------------------------------------------------------------------------

#[test]
fn command_names_common_part_then_list_then_step() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    slash(app, "/w");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ws ", "several: the common part");
    assert_eq!(phase(app), Some(Phase::Common));
    key(app, KeyCode::Tab);
    assert_eq!(phase(app), Some(Phase::List(0)), "second tab: the list");
    let listed = titles(app);
    assert_eq!(listed.len(), 5, "{listed:?}");
    assert!(listed[0].starts_with("/ws add"), "{listed:?}");
    assert_eq!(
        input(app),
        "/ws add ",
        "the input shows the selected candidate"
    );
    key(app, KeyCode::Tab);
    assert_eq!(phase(app), Some(Phase::List(1)));
    back_tab(app);
    back_tab(app);
    assert_eq!(phase(app), Some(Phase::List(4)), "shift+tab wraps");
    // Esc closes the list, not the palette, and restores the text from before it.
    key(app, KeyCode::Esc);
    assert_eq!(app.focus, Focus::Palette);
    assert_eq!(input(app), "/ws ");
    assert_eq!(phase(app), None);
    key(app, KeyCode::Esc);
    assert!(app.palette.is_none(), "a second esc closes the palette");
}

#[test]
fn one_match_completes_and_a_bare_slash_opens_the_list_at_once() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    slash(app, "/rel");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/reload");
    assert_eq!(phase(app), None);
    crate::slash_tests::set(app, "/");
    key(app, KeyCode::Tab);
    assert_eq!(phase(app), Some(Phase::List(0)), "no common part: the list");
}

// ---- paths ----------------------------------------------------------------------------------

#[test]
fn ws_add_folders_common_list_take() {
    let mut f = Fx::new("vr-ide");
    let roots_before = f.app.workspace.roots().len();
    let app = &mut f.app;
    slash(app, "/ws add ../fix");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ws add ../fixecom");
    assert_eq!(phase(app), Some(Phase::Common));
    // While the common part shows, the row is the command's and the input says how many.
    assert_eq!(titles(app), ["/ws add <folder>"]);
    key(app, KeyCode::Tab);
    assert_eq!(titles(app), ["../fixecom/", "../fixecom-admin/"]);
    key(app, KeyCode::Down);
    assert_eq!(input(app), "/ws add ../fixecom-admin/", "down steps too");
    // Enter takes it and closes the list; nothing runs.
    key(app, KeyCode::Enter);
    assert_eq!(app.focus, Focus::Palette);
    assert_eq!(phase(app), None);
    assert_eq!(input(app), "/ws add ../fixecom-admin/");
    assert_eq!(app.workspace.roots().len(), roots_before);
}

#[test]
fn one_folder_ends_with_a_slash_hidden_only_after_a_dot() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    slash(app, "/ws add ../sh");
    key(app, KeyCode::Tab);
    assert_eq!(
        input(app),
        "/ws add ../shop/",
        "a folder gets a slash, no space"
    );
    crate::slash_tests::set(app, "/ws add ../");
    key(app, KeyCode::Tab);
    assert_eq!(phase(app), Some(Phase::List(0)));
    assert_eq!(titles(app).len(), 7, "no .hidden: {:?}", titles(app));
    key(app, KeyCode::Esc);
    crate::slash_tests::set(app, "/ws add ../.h");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ws add ../.hidden/");
}

#[test]
fn ws_open_lists_folders_and_workspace_files_only() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    slash(app, "/ws open ../t");
    key(app, KeyCode::Tab);
    assert_eq!(
        input(app),
        "/ws open ../team.vr-workspace",
        "todo.txt is not offered"
    );
}

#[test]
fn typing_closes_the_list_and_edits() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    slash(app, "/ws add ../");
    key(app, KeyCode::Tab);
    assert_eq!(phase(app), Some(Phase::List(0)));
    typ(app, "x");
    assert_eq!(phase(app), None);
    assert_eq!(input(app), "/ws add ../api-gateway/x");
}

#[test]
fn listing_is_bounded_and_unsafe_names_are_skipped() {
    let mut f = Fx::new("vr-ide");
    let many = f.dev().join("many");
    std::fs::create_dir(&many).unwrap();
    for i in 0..1500 {
        std::fs::write(many.join(format!("f{i:04}")), "").unwrap();
    }
    std::fs::write(f.dev().join("vr-ide/bad\u{1b}[2Jname"), "").unwrap();
    let app = &mut f.app;
    slash(app, "/save-as ../many/f");
    key(app, KeyCode::Tab);
    let p = app.palette.as_ref().unwrap();
    assert!(p.items.len() <= 100, "shown: {}", p.items.len());
    assert!(
        p.dir_cache.as_ref().unwrap().len() <= 1000,
        "read at most 1000 entries"
    );
    crate::slash_tests::set(app, "/save-as ba");
    key(app, KeyCode::Tab);
    assert_eq!(
        input(app),
        "/save-as ba",
        "a name with control characters is not offered"
    );
    assert!(titles(app).iter().all(|t| !t.contains('\u{1b}')));
}

// ---- /ssh -----------------------------------------------------------------------------------

#[test]
fn ssh_classifier_positions() {
    use SshSlot::*;
    assert_eq!(completion::ssh_slot_at_end(""), Host(0));
    assert_eq!(completion::ssh_slot_at_end("ro"), Host(0));
    assert_eq!(completion::ssh_slot_at_end("-i "), Path(3));
    assert_eq!(completion::ssh_slot_at_end("-i cred"), Path(3));
    assert_eq!(completion::ssh_slot_at_end("-icred"), Path(2));
    assert_eq!(completion::ssh_slot_at_end("-F c"), Path(3));
    assert_eq!(completion::ssh_slot_at_end("-o IdentityFile=c"), Path(16));
    assert_eq!(completion::ssh_slot_at_end("-oidentityfile=c"), Path(15));
    assert_eq!(completion::ssh_slot_at_end("-o Port=2"), Other);
    assert_eq!(completion::ssh_slot_at_end("-p 22"), Other);
    assert_eq!(completion::ssh_slot_at_end("-p 22 h"), Host(6));
    assert_eq!(completion::ssh_slot_at_end("-i k -A ro"), Host(8));
    assert_eq!(
        completion::ssh_slot_at_end("host ls"),
        Other,
        "remote command"
    );
    assert_eq!(completion::ssh_slot_at_end("-v"), Other);
    assert_eq!(
        completion::ssh_slot_at_end("-i"),
        Other,
        "the option itself"
    );
    assert_eq!(completion::ssh_slot_at_end("-i my\\ k"), Path(3));
}

#[test]
fn ssh_identity_completes_files_of_the_active_files_root() {
    let mut f = Fx::new("fixecom");
    let app = &mut f.app;
    slash(app, "/ssh -i cred");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ssh -i credentials/");
    key(app, KeyCode::Tab);
    assert_eq!(
        input(app),
        "/ssh -i credentials/id_fixecom",
        "the common part"
    );
    key(app, KeyCode::Tab);
    assert_eq!(
        titles(app),
        [
            "credentials/id_fixecom",
            "credentials/id_fixecom.pub",
            "credentials/id_fixecom_deploy"
        ]
    );
    key(app, KeyCode::Esc);
    crate::slash_tests::set(app, "/ssh -i credentials/id_fixecom.");
    key(app, KeyCode::Tab);
    assert_eq!(
        input(app),
        "/ssh -i credentials/id_fixecom.pub ",
        "a finished argument gets a space"
    );
    crate::slash_tests::set(app, "/ssh -o IdentityFile=credentials/id_fixecom_");
    key(app, KeyCode::Tab);
    assert_eq!(
        input(app),
        "/ssh -o IdentityFile=credentials/id_fixecom_deploy "
    );
}

#[test]
fn ssh_path_names_with_spaces_are_escaped() {
    let mut f = Fx::new("fixecom");
    std::fs::write(f.dev().join("fixecom/my key"), "").unwrap();
    let app = &mut f.app;
    slash(app, "/ssh -i my");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ssh -i my\\ key ");
}

#[test]
fn ssh_destination_completes_config_hosts_and_recent_targets() {
    let mut f = Fx::new("fixecom");
    let app = &mut f.app;
    slash(app, "/ssh -i credentials/id_fixecom b");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ssh -i credentials/id_fixecom bastion ");
    crate::slash_tests::set(app, "/ssh -i credentials/id_fixecom ");
    key(app, KeyCode::Tab);
    assert_eq!(phase(app), Some(Phase::List(0)));
    assert_eq!(
        titles(app),
        [
            "root@app.fixecom.vn",
            "app.fixecom.vn",
            "app-staging",
            "bastion",
            "git.example.com"
        ]
    );
    let keys: Vec<String> = app
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.keys.clone())
        .collect();
    assert_eq!(keys[0], "recent");
    assert_eq!(keys[1], "~/.ssh/config");
    // Past the destination (the remote command) nothing is completed.
    key(app, KeyCode::Esc);
    crate::slash_tests::set(app, "/ssh bastion ls ");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ssh bastion ls ");
    assert_eq!(phase(app), None);
}

// ---- /run, /ai ------------------------------------------------------------------------------

#[test]
fn run_completes_file_arguments_not_the_program() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    slash(app, "/run cat REA");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/run cat README.md ");
    crate::slash_tests::set(app, "/run REA");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/run REA", "the first word is the program");
}

#[test]
fn ai_completes_tool_names() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    slash(app, "/ai cl");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ai claude ");
    crate::slash_tests::set(app, "/ai ");
    key(app, KeyCode::Tab);
    let names: Vec<String> = app
        .settings
        .ai_tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert!(names.len() > 1, "{names:?}");
    assert_eq!(titles(app), names, "the ai_tools setting, in order");
    assert_eq!(completion::ai_tool_names(app), names);
}

#[test]
fn ai_completes_the_names_of_the_ai_tools_setting() {
    let mut f = Fx::new("vr-ide");
    let mut agent = f.app.settings.ai_tools[0].clone();
    agent.name = "cursor-agent".to_string();
    f.app.settings.ai_tools.push(agent);
    let app = &mut f.app;
    slash(app, "/ai cu");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ai cursor-agent ");
}

/// `/ai <tool> @` completes that tool's accounts (and `@default`); a tool without accounts and a
/// third word complete nothing (D11).
#[test]
fn ai_completes_the_accounts_of_the_tool() {
    let mut f = Fx::new("vr-ide");
    let state = tempfile::tempdir().unwrap();
    crate::ai::accounts::create(state.path(), "claude", "work").unwrap();
    crate::ai::accounts::create(state.path(), "claude", "alt").unwrap();
    crate::ai::accounts::create(state.path(), "codex", "cx").unwrap();
    f.app.ai.state_dir = Some(state.path().to_path_buf());
    let app = &mut f.app;
    slash(app, "/ai claude @w");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ai claude @work");
    crate::slash_tests::set(app, "/ai claude ");
    key(app, KeyCode::Tab);
    key(app, KeyCode::Tab);
    assert_eq!(titles(app), ["@default", "@alt", "@work"]);
    crate::slash_tests::set(app, "/ai codex @");
    key(app, KeyCode::Tab);
    assert_eq!(titles(app), ["@default", "@cx"]);
    for none in ["/ai gemini @", "/ai claude @work x", "/ai claude w"] {
        crate::slash_tests::set(app, none);
        key(app, KeyCode::Tab);
        assert_eq!(input(app), none, "{none}");
    }
}

#[test]
fn existing_palette_keys_still_work() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    // An explicit highlight is what Tab takes.
    slash(app, "/term");
    key(app, KeyCode::Down);
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/term bottom");
    // Tab outside slash mode does nothing and keeps the palette.
    crate::slash_tests::set(app, "theme");
    key(app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Palette);
    assert_eq!(input(app), "theme");
}

// ---- rendering ------------------------------------------------------------------------------

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|fr| crate::ui::render(fr, app)).unwrap();
    t
}

/// The frame with the temp folder's random part masked (same length, so nothing moves).
fn frame(t: &Terminal<TestBackend>, tmp: &Path) -> String {
    let tmp = tmp.to_string_lossy().into_owned();
    let mask = format!("/tmp/vrT4{}", "X".repeat(tmp.len() - "/tmp/vrT4".len()));
    format!("{}", t.backend()).replace(&tmp, &mask)
}

fn row(t: &Terminal<TestBackend>, y: u16) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

#[test]
fn renders_common_part_and_lists() {
    for (w, h) in [(80, 24), (120, 40), (40, 12)] {
        let mut f = Fx::new("vr-ide");
        let tmp = f.dir.path().to_path_buf();
        let app = &mut f.app;
        slash(app, "/ws add ../fix");
        key(app, KeyCode::Tab);
        let t = draw(app, w, h);
        let area = app.palette.as_ref().unwrap().area;
        let input_row = row(&t, area.y + 1);
        // The typed command stays readable; a narrow box shortens the tag.
        assert!(input_row.contains("> /ws add ../fixecom"), "{input_row}");
        let tag = if w < 60 { "tab (2)" } else { "tab: list (2)" };
        assert!(input_row.contains(tag), "{input_row}");
        insta::assert_snapshot!(format!("palette_tab_prefix_{w}x{h}"), frame(&t, &tmp));
        crate::slash_tests::set(app, "/ws add ../");
        key(app, KeyCode::Tab);
        key(app, KeyCode::Tab);
        key(app, KeyCode::Tab);
        let t = draw(app, w, h);
        assert!(
            row(&t, area.y + 1).contains("3/7"),
            "{}",
            row(&t, area.y + 1)
        );
        insta::assert_snapshot!(format!("palette_tab_list_{w}x{h}"), frame(&t, &tmp));
    }
}

#[test]
fn renders_ssh_path_and_host_lists() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut f = Fx::new("fixecom");
        let tmp = f.dir.path().to_path_buf();
        let app = &mut f.app;
        slash(app, "/ssh -i credentials/id_fixecom");
        key(app, KeyCode::Tab);
        let t = draw(app, w, h);
        let text = frame(&t, &tmp);
        assert!(
            text.contains("files in ~/DEV/fixecom (workspace folder of the active file)"),
            "{text}"
        );
        insta::assert_snapshot!(format!("palette_ssh_path_{w}x{h}"), text);
        key(app, KeyCode::Esc);
        crate::slash_tests::set(app, "/ssh -i credentials/id_fixecom ");
        key(app, KeyCode::Tab);
        let t = draw(app, w, h);
        let text = frame(&t, &tmp);
        assert!(text.contains("runs: ssh -i /tmp/vrT4"), "{text}");
        insta::assert_snapshot!(format!("palette_ssh_host_{w}x{h}"), text);
    }
}

#[test]
fn a_completed_destination_gets_a_space_and_shows_what_runs() {
    let mut f = Fx::new("fixecom");
    let tmp = f.dir.path().to_path_buf();
    let app = &mut f.app;
    slash(app, "/ssh git");
    key(app, KeyCode::Tab);
    assert_eq!(input(app), "/ssh git.example.com ");
    let t = draw(app, 80, 24);
    let text = frame(&t, &tmp);
    assert!(text.contains("runs: ssh git.example.com"), "{text}");
    // The list is not open: Enter acts as usual and the next edit drops the line.
    assert!(!completion::list_open(app));
    typ(app, "x");
    assert_eq!(phase(app), None);
}

/// SEC-16: path completion lists nothing inside an account folder (not even names), while a
/// sibling folder still completes.
#[test]
fn sec16_path_completion_never_lists_an_account_folder() {
    let mut f = Fx::new("vr-ide");
    let state = tempfile::tempdir().unwrap();
    crate::ai::accounts::create(state.path(), "claude", "work").unwrap();
    let work = crate::ai::accounts::path_of(state.path(), "work").unwrap();
    std::fs::write(work.join(".credentials.json"), "").unwrap();
    std::fs::write(work.join("token"), "").unwrap();
    std::fs::create_dir(state.path().join("otherdir")).unwrap();
    f.app.ai.state_dir = Some(state.path().to_path_buf());
    let app = &mut f.app;
    let s = state.path().display().to_string();
    slash(app, "/open");
    for cmd in ["/open", "/ws add", "/ssh -i"] {
        let typed = format!("{cmd} {s}/accounts/work/");
        crate::slash_tests::set(app, &typed);
        key(app, KeyCode::Tab);
        let leaked = |app: &App| {
            let all = [vec![input(app)], titles(app)].concat();
            all.iter()
                .any(|t| t.contains("token") || t.contains(".credentials"))
        };
        assert!(!leaked(app), "{typed}");
        let dotted = format!("{typed}.");
        crate::slash_tests::set(app, &dotted);
        key(app, KeyCode::Tab);
        assert!(!leaked(app), "{dotted}");
    }
    // Through a symlink as well.
    let link = state.path().join("lnk");
    std::os::unix::fs::symlink(work, &link).unwrap();
    let typed = format!("/open {s}/lnk/to");
    crate::slash_tests::set(app, &typed);
    key(app, KeyCode::Tab);
    assert_eq!(input(app), typed);
    // A folder next to the accounts still completes.
    key(app, KeyCode::Esc);
    slash(app, &format!("/ws add {s}/oth"));
    key(app, KeyCode::Tab);
    assert_eq!(input(app), format!("/ws add {s}/otherdir/"));
}

/// SEC-16: the `accounts/` folder itself is never listed either (the names are the accounts),
/// by any command that completes a path.
#[test]
fn sec16_the_accounts_folder_itself_is_never_listed() {
    let mut f = Fx::new("vr-ide");
    let state = tempfile::tempdir().unwrap();
    crate::ai::accounts::create(state.path(), "claude", "work").unwrap();
    crate::ai::accounts::create(state.path(), "claude", "other").unwrap();
    f.app.ai.state_dir = Some(state.path().to_path_buf());
    let app = &mut f.app;
    let s = state.path().display().to_string();
    for cmd in ["/ws add", "/open", "/ssh -i", "/run x"] {
        for tail in ["accounts/", "accounts/w"] {
            let typed = format!("{cmd} {s}/{tail}");
            key(app, KeyCode::Esc);
            slash(app, &typed);
            key(app, KeyCode::Tab);
            key(app, KeyCode::Tab);
            assert_eq!(input(app), typed, "{cmd} {tail}: Tab fills nothing");
            let listed = titles(app).join("|");
            assert!(
                !listed.contains("work") && !listed.contains("other"),
                "{cmd} {tail}: {listed}"
            );
        }
    }
}

/// Tab with nothing to complete leaves what was typed in `/ssh`.
#[test]
fn tab_with_no_candidates_keeps_the_typed_ssh_line() {
    let mut f = Fx::new("vr-ide");
    let app = &mut f.app;
    for typed in ["/ssh -i /nonexistent-zz/", "/ssh -o Foo=bar"] {
        key(app, KeyCode::Esc);
        slash(app, typed);
        key(app, KeyCode::Tab);
        assert_eq!(input(app), typed);
    }
}
