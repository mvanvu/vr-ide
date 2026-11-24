# Design QA T17 — welcome redesign options A / B / C

Verdict: PASS (showable). No blocker. Two majors and several minors are listed below. None of them blocks showing the options. Fix the B items while building B.
Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Method: `qa-artifacts/T17/qa/parse.py` reads each .ans cell by cell. It renders to a fixed 1ch HTML grid, and I screenshotted that with headless Chrome. Screenshots are `qa-artifacts/T17/qa/{a,b,c}-{120x40,80x24,40x12}.png`. Chrome's block glyphs are slightly wider, so alignment was judged from the text dump.

## Structure check of the .ans files (all 9 pass)
| File | Rows x cols | Every line ends `ESC[0m` | bg on every cell | Stray escapes | Wide chars |
|---|---|---|---|---|---|
| a/b/c 120x40 | 40 x 120 exact | yes | yes | none | none |
| a/b/c 80x24 | 24 x 80 exact | yes | yes | none | none |
| a/b/c 40x12 | 12 x 40 exact | yes | yes | none | none |
Only SGR 0, 1, 22, 38;2 and 48;2 are used. A trailing newline is present. show.sh prints a header line and cats the file, so a terminal needs rows+1 lines at 40x12 and 80x24 (the header scrolls the tab bar off if the terminal is exactly the target size). Minor: the header text says "needs at least", so this is not a defect.

## Contrast
The notes are right. The values in the notes match my computation from the real fg/bg pairs in the files: comment 3.94 and line 1.54 on bg, and every other colour is above 4.5.
Finding: the comment colour (3.94) carries words, not only separators. It colours the `60 s` label in B 120 (minor, see F5).

## Per option, per size
| | 120x40 | 80x24 | 40x12 |
|---|---|---|---|
| A card | Calm and clean. The card is small in the middle (78x22 on a 120x40 screen) and there is a lot of empty space around it. Monitor is a 3-row strip, per-core is a tiny glyph row with no labels. Pass | Best-looking size of A. Card fills the screen, everything legible. Pass | Clean. Border gone, CPU spark + RAM bar kept, 4 keys, 1 recent. Pass |
| B dashboard | Strongest. 6-row CPU history, per-core bars, RAM/SWP, load, uptime, own process. Bottom 40% of both columns is empty. F1 and F2 apply. Pass with fixes | Good. The right column shows every metric as text and bars. Per-core row is unlabeled (F6). Pass | Good. Header, CPU spark+%, RAM bar+%, 4 keys. Divider and extra rows gone cleanly. Pass |
| C launcher | Logo reads as VR-IDE and the menu is clear. Monitor strip is cramped, and the separators do not line up between its two rows (F3). Logo costs 6 rows. Pass with fixes | Dense monitor strip, row 2 uses 78 of 80 cols and `vr-ide 1.2% 48M` is unlabeled (F4). Pass with fixes | Wordmark + 4 menu rows + CPU/RAM. Loses version/root/recent but still tidy. Pass |

## Findings
| ID | Sev | Where | Problem | Fix |
|---|---|---|---|---|
| F1 | major | b-120x40 VR-IDE block | The RSS bar fills about 40%. 48 MiB of 15.5 GiB is 0.3%. The bar invents a scale the user cannot read and implies vr-ide is using a big share of RAM. | Drop the bar and show the number plus a small history sparkline, or scale it against total RAM and let it be nearly empty. |
| F2 | major | all options, tab row and status bar | The chrome does not match the build. The mockups draw a pink bold "Welcome" tab (the active look) and a READ badge in the status bar. The build (D188, `tabs.rs:16`, `status_bar.rs:75-78`) draws only inactive document tabs while welcome is up and the status bar shows only ` <root>` with no badge. The user would be choosing from a picture that shows UI that does not exist. | Either redraw the chrome as built (no Welcome tab, root-only status bar) or state in the notes that the badge and tab are proposed changes. |
| F3 | minor | c-120x40, c-80x24 monitor | Column separators `│` of row 1 and row 2 are at different columns (80x24: row 1 at col 19, row 2 at col 22), so the strip looks staggered. | Put both rows on one column grid. |
| F4 | minor | c-80x24 row 2 | `vr-ide 1.2% 48M` has no cpu/rss labels. The row is at 78 of 80 columns. | Add `cpu`/`rss` labels or move vr-ide to the 120 layout only. |
| F5 | minor | b-120x40 `60 s` | Text in the comment colour (3.94:1) is below 4.5:1. | Use dim. |
| F6 | minor | b-80x24 and a-120x40 per-core row | The mini bars have no core index and no values. Label `core` / `CPU/core` is lowercase or longer than the `CPU` label beside it. | Label as `CPU/c` or keep `CORE` uppercase. At 80 it is acceptable. |
| F7 | minor | b-120x40 right column | Right edges are not shared. Core percentages end at x=116, LOAD/uptime at 136 and RAM/SWP % at 145 (screenshot px). | Use one right edge for all values. |
| F8 | minor | a-80x24 and a-120x40 | SWP is paired with CPU on the left and LOAD with RAM on the right. Swap belongs next to RAM. `up` and `cpu`/`rss` are lowercase suffixes, and the other labels are uppercase prefixes. | Put RAM and SWP in one column and CPU and LOAD in the other. Prefix labels consistently. |
| F9 | minor | b-80x24 vs b-120x40 | RAM is `61% 9.4/15.5G` at 80 and `9.4/15.5 GiB 61%` at 120. The order and unit differ. | Use one order and one unit. |
| F10 | minor | all | The Welcome hint row shows `ctrl+shift+w`. D190 added `ctrl+alt+w` for legacy terminals. | Show the key the build shows. |

## Ranking
1. B. It is the only layout where the monitor is a real dashboard. It has no decoration, all keys visible, and the best 120 use. It degrades most simply.
2. A. It looks calmest and most polished at 80x24. The card border costs 4 rows and 4 columns against the density taste, and the monitor is a cramped strip.
3. C. The nvim launcher feel is familiar. The 5-row logo is decoration that costs space (design_taste). The monitor is thin and dense at 80.

B is the right recommendation. Fix F1 and F2 before the user picks, if cheap. F7 to F9 can go in the implement step.

## Rule check
No rule packs were assigned to this design step. n-o.

## r1 re-check
Verdict: PASS. Re-ran `parse.py` on the 9 .ans files plus the 3 `*-80x24-notabs.ans` (new `p2.py`) and regenerated screenshots in `qa-artifacts/T17/qa/`.
- Structure: all 12 files are exact rows x cols, every line ends `ESC[0m`, a background is on every cell, no stray escapes. The only low-contrast pairs left are the decorative line/comment colour (rules, card border, `·`, `│`), so the `60 s` text issue (F5) is gone.
- F1 closed: B 120 shows `RSS 48 MiB  0.3% of RAM` as a number only. Every bar left is a true percentage.
- F2 closed: the tab row shows document tabs only, with none active. The status bar is root-only, with no badge. The notabs files show an empty tab row.
- F3 to F10: confirmed in the screenshots. C monitor separators are on one column. Labels are uppercase prefixes. Cores are indexed (`CORE 0▄ 1▂`, `C0..C7`). B has one right edge. RAM/SWP are stacked together. `ctrl+alt+w / ctrl+shift+w` is shown.
- Known blemish (minor, accepted): B 80x24 `ctrl+alt+w / ctrl+shift+w  Welcome` overflows the 12-wide key column, so its label sits left of the other labels. It still fits the 38-wide left column and the divider is intact. Suggest widening the key column or moving the alternate key to its own line.
- Regressions: none. The ranking stays B, A, C.
