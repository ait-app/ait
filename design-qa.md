# Task board reference implementation — visual QA

Result: **passed for the task-board scope**, with the intentional Ait adaptations below. GitHub attachment upload is blocked separately: the GitHub connector has no upload endpoint and Cua Driver reports no available browser windows. Screenshots remain outside Git; none are included in the code commit.

## Target and source

The selected target is the user's 1920 × 1080 Better Paseo cover image in this conversation: iPad task board with a collapsed Running column, inline waiting/review actions, and a phone showing three running cards. The marketing background and device frames are not application UI.

Supplemental official source: <https://paseo.sh/plugins/roundreddot/better-paseo> and <https://github.com/RoundRedDot/better-paseo>. The current release images are newer than the selected cover and are supplemental only:

- `/tmp/ait-task-overview-preview/reference-ipad.png`
- `/tmp/ait-task-overview-preview/reference-mobile.png`

The official iPad image and rendered tablet screenshot were inspected together in one comparison input; the official phone image and rendered phone screenshot were inspected together in another. The selected cover remains the authority for compactness and the collapsed-column composition.

## Rendered evidence

All captures come from the existing authorized Playwright browser, running the real client against a local test daemon with offline provider fixtures. These are rendered UI screenshots, not design mockups.

| Capture           | Viewport and state                                                                           | File                                                        |
| ----------------- | -------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| Desktop           | 1717 × 916, light, nine root tasks plus nested tasks, all columns                            | `/tmp/ait-task-overview-preview/inline-desktop-final.png`   |
| Collapsed desktop | 1717 × 916, Running collapsed to 40 px                                                       | `/tmp/ait-task-overview-preview/inline-collapsed-final.png` |
| Reference tablet  | 1200 × 820, light, nine tasks: three waiting, three running, three review; Running collapsed | `/tmp/ait-task-overview-preview/inline-tablet-final.png`    |
| Reference phone   | 390 × 844, light, Running selected with three cards                                          | `/tmp/ait-task-overview-preview/inline-phone-final.png`     |
| Dark desktop      | 2048 × 1152, 32 long-title tasks, offline host notice and detail panel                       | `/tmp/ait-task-overview-preview/inline-dark-final.png`      |
| Detail            | 1717 × 916, approval, recent reply and child navigation                                      | `/tmp/ait-task-overview-preview/inline-detail-final.png`    |

## Comparison history and fixes

1. Replaced the oversized title/filter area with a 48 px toolbar. Cards now use 14 px titles, 12 px metadata, provider/effort and branch pills, project icons, status beside the title, and inline actions.
2. Compared the first tablet render with the source. A collapsed lane had zero computed width due to flex basis; an explicit 40 px minimum fixed it. The test now checks the collapsed lane width and restoration.
3. Checked question and command cards. Questions show parsed question text and open the existing answer form; commands retain the full provider choice set and expose complete details on click. Unknown command input fields remain visible in the summary instead of being silently discarded.
4. Compared the phone render with the source. Three running cards fit together; stop/open remain available. Added a geometry check that expanded search cannot overlap the toolbar controls.
5. Inspected the narrow tablet review lane. The extra archive button forced Open to wrap vertically. Review cards now show Mark as read + Open; archive appears after marking read and remains available in details. Open has an 84 px minimum. The final screenshot and button-height check confirm a single-line label.
6. Inspected final light and dark renders, long titles, side panel, collapsed lane, empty Done state, offline notice, and mobile tabs. No horizontal page overflow or clipped action labels remain in the tested viewports.

## Visual and behavior checklist

- **Layout / spacing:** compact toolbar; four independent task lists; collapsible columns; no large page hero; mobile tabs; details inline when space permits and in a sheet otherwise.
- **Typography:** uses existing Ait typography tokens and the user's interface font. SF/system font rendering from the reference is not bundled or forced; Linux/browser font fallback can differ. Long project names and titles have bounded wrapping.
- **Colors:** uses Ait's light/dark surfaces, running blue, waiting amber, review/done green and failure red. Provider pills are neutral to match existing Ait styles rather than introducing a separate provider-color system.
- **Icons / assets:** reuses ProjectIconView, provider icons and Lucide. No raster marketing or device-frame assets are shipped.
- **Copy / data:** Chinese labels follow existing Ait localization. Offline test model names, permissions and localhost labels are test data. Provider-defined action labels are preserved verbatim. Ait host metadata and native read-only capabilities are retained.
- **State:** failed tasks appear under Review; Waiting is reserved for input/approval. Sidebar displays waiting/running/review counts. Child aggregation remains intact when child rows are hidden.
- **Actions:** inline allow/deny preserve provider action IDs; answer opens the existing form; stop/read/archive use the same actions and pending locks as details; Open navigates to the real conversation. Offline and observed task restrictions remain in force.

## Verification

- 73 focused unit tests passed across the task-board model/hook, permissions, question forms and translations.
- Four Playwright task-board scenarios passed, covering real daemon updates, inline approvals, stop/read/archive/open, questions, child navigation, filters/search, collapse/expand, responsive geometry and offline handling.
- Mobile TypeScript, desktop main build, repository oxlint/oxfmt, UI lint checks, documentation checks and `git diff --check` passed.
- No Rust code changed and no Rust tests were run.
- Native touch gestures, physical iPad/iPhone rendering, the reference plugin's chat timeline and unrelated plugin features are outside this board change and are not claimed as verified.
