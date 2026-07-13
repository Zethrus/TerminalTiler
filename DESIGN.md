# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-07-13 (premium control polish: brand switch/scrollbar/combo recipes, chip family contract, flat toolbar + tab treatment, stylesheet moved to USER priority so desktop themes cannot re-skin the app)
- Primary product surfaces: workspace launch dashboard, workspace creation/edit wizard, saved preset/workspace cards, saved Kanban board cards, active Kanban board tabs, task detail dialogs, agent run panel, tile editor, active workspace tabs.
- Evidence reviewed: `README.md`, `docs/core-boundary.md`, `docs/kanban-board.md`, `src/ui/launch_screen.rs`, `src/ui/window.rs`, `src/ui/workspace_view.rs`, `src/ui/board_view.rs`, `src/ui/task_detail_dialog.rs`, `src/ui/agent_setup_dialog.rs`, `src/model/preset.rs`, `src/model/layout.rs`, `src/model/board.rs`, `src/services/agent_orchestrator.rs`, `resources/style.css`.

## Brand
- Personality: focused, command-center, native desktop, powerful but calm.
- Trust signals: local-first presets and boards, explicit workspace/project paths, clear launch/edit/open actions, reversible navigation, visible validation, task progress written to project-owned files.
- Avoid: one giant scroll form, hidden destructive actions, cloud/external-only assumptions in Core, decorative UI that obscures workflow setup.

## Product goals
- Goals: make creating a terminal workspace feel guided; make existing saved workspaces and Kanban boards easy to reopen; make editing a saved setup discoverable; preserve advanced tile customization without forcing users to confront every field at once; make AI-agent work inspectable through board notes, knowledge, and live terminals.
- Non-goals: adding external synchronization, accounts, new dependencies, or a separate design-system framework.
- Success signals: users see saved workspaces, saved Kanban boards, or prominent new-setup actions first; each wizard screen has one clear decision; saved presets and board shortcuts can be opened, edited, and deleted locally; task progress remains visible even when an agent is doing the work.

## Personas and jobs
- Primary personas: developers, AI-agent operators, reviewers, release operators, Windows/WSL users needing repeatable terminal layouts.
- User jobs: reopen a known workspace quickly; create a new layout from a template; tune tile roles/commands/directories; save and later edit a preset; open a project task board; dispatch or review agent work; inspect task context before deciding completion.
- Key contexts of use: local desktop app launch, project directory setup, iterative workspace reconfiguration between tasks, task execution and review inside a project board.

## Information architecture
- Primary navigation: app tab strip for workspace and board tabs; launch dashboard inside each empty workspace tab; wizard navigation for workspace and board setup steps.
- Core routes/screens: saved workspace/board dashboard, new/edit workspace wizard, new/edit Kanban board wizard, active workspace runtime, active Kanban board, task detail dialog, agent setup dialog, settings, assets manager.
- Content hierarchy: overview and quick actions first; path/name before template or board review; appearance before tile detail; launch/open/save actions stay in the wizard footer; board columns stay scannable before task details.

## Design principles
- Principle 1: progressive disclosure — show one setup decision at a time and keep advanced tile fields in the final step.
- Principle 2: action clarity — “Open”, “Edit”, and “New Workspace Layout” should be explicit and visually distinct.
- Principle 3: inspectable automation — agent actions should leave visible board state through assignee chips, notes, knowledge entries, review state, and live terminal rows.
- Tradeoffs: the wizard adds clicks compared with a long form, but reduces scanning burden and makes saved workspaces easier to understand.

## Visual language
- Color: preserve the existing dark command-center palette with amber accent highlights and light-mode overrides. Accent is used with restraint — reserve full-strength amber for one primary action per region; the active tab is muted copper plus a thin amber underline rather than a loud copper gradient; secondary controls avoid amber rim-glow.
- Typography: keep current GTK/libadwaita typography classes on the existing font stack. Type scale: display 22 · section 14–15 · card/tile-title 12–13 · body 13 · meta 10–11 · micro 9; max weight 700. Eyebrows/step indicators are 9–10px / 600 / uppercase with 0.14em letter-spacing.
- Spacing/layout rhythm: 4px-based scale (4 · 6 · 8 · 12 · 16 · 20) applied as literals (GTK4 CSS has no length variables). Card surfaces use ~12px panel padding (Standard); wizard body limited to the active step. GTK `Box` spacings follow the same steps.
- Shape/radius/elevation: radius scale control 8 · chip 999 · card 12 · panel 14 (`profile-compact` stays squared; `workspace-summary` stays squared, with dense runtime-toolbar controls using crisp 2px corners). Elevation is soft — 1px hairline borders (`alpha(@tt_white, 0.06–0.08)`) do primary separation, shadows are secondary (e1 0.12 · e2 0.16 · e3 0.22). Alert/confirm dialog sheets use panel radius 14, a hairline outline, and the popover overlay shadow. Pill CTAs and selected-card borders retained. The numeric scale's single source of truth is the header comment in `resources/style.css` mirrored here.
- Motion: native GTK stack transitions are acceptable; avoid long or distracting animations. Interactive cards lift `translateY(-1px)` on hover at 140ms ease.
- Control recipes (2026-07 polish): switches use a quiet dark trough with an amber→copper gradient when checked and an ink slider — never stock Adwaita blue; scrollbars are overlay-style 6px pill sliders on a transparent trough; combo arrows rest neutral and take amber only on hover/open, with amber-tint selected rows in the popup; entry focus is a calm amber border (`alpha(@tt_amber, 0.38)`) with no glow halo.
- Imagery/iconography: use symbolic GTK icons only; no new bitmap assets.

## Components
- Existing components to reuse: `config-panel`, `preset-card`, `preset-card-compact`, `control-strip`, `pill-button`, `primary-cta-button`, `secondary-button`, `surface-button`, `status-chip`, `tile-editor-row`.
- Chip family contract: all pill chips share shape (radius 999, padding 3px 8px, 10px/600) with a micro tier (9px, 0.04em tracking, `settings-meta-chip`/`launch-meta-chip`) and four tones — neutral (`status-chip` base: ink 0.82), muted (ink 0.56: `muted-chip`, `saved-workspace-tile-chip`), accent (amber tint: `saved-board-kind-chip`), semantic (`companion-status-chip.is-*` colors). Intentional exceptions: `settings-shortcut-chip` (mono key-cap) and the squared Kanban board chips, which follow the board's crisp-corner family.
- Premium modal scaffold: alert/confirm/notice dialogs are composed by `dialog_chrome::PremiumModal` (`src/ui/dialog_chrome.rs`) using the `premium-modal-*` classes (surface sheet, icon chip with danger/amber accents, eyebrow, heading, body, warning callout, action row); every alert surface should use it instead of stock `adw::MessageDialog`.
- New/changed components: launch dashboard, saved workspace action cards, saved Kanban board cards, workspace wizard stepper, board wizard stepper, board columns/cards, task detail tabs, agent setup dialog, agent run rows, wizard footer navigation, step containers.
- Variants and states: selected template/preset, saved board shortcut, disabled Back on first step, primary Next vs Launch/Open on final step, built-in preset “Save Copy” state, invalid path state, Kanban status column, drag target, active agent run, completed/cancelled agent run.
- Token/component ownership: `resources/style.css` owns visual tokens/classes; `src/ui/launch_screen.rs` owns launch/wizard composition; `src/ui/board_view.rs` and `src/ui/board_chrome.rs` own board composition.

### Button role contract
- `primary-cta-button` / GTK `suggested-action`: warm cream, highest-emphasis CTA for launch/open/create actions (`Launch Workspace`, `Open`, `Open Kanban Board`, `New Workspace Layout`); compact variants keep the same amber rim, dark text/icons, and dimensional shadow.
- `secondary-button`: dark glass support action for reversible navigation and editing (`Back`, `Update Preset`, `Edit`, `Workspaces`, `New Kanban Board`) with subtle amber border/highlight.
- `ghost-link-button` / `pill-button.flat`: low-emphasis transparent/dark action for exits such as `Close Tab`; readable without competing with the primary CTA.
- `destructive-button` / GTK `destructive-action`: compact dark red-accent risk action for delete/close icon buttons; destructive actions should remain explicit and tooltip-backed when icon-only.
- `surface-button` / `surface-button-icon`: tile-editor and runtime surface controls; use the same dark glass token family at a smaller square/labeled size. The workspace summary toolbar uses the densest scoped variant: 24–26px high, 10–11px text, icon-first utility actions, and visible labels only for dynamic/status context.
- Focus and disabled states: keyboard focus uses a visible amber ring; disabled buttons are muted but intentional role states, never generic washed-out platform gray.
- Modal action order: destructive confirms place Cancel (secondary, default) before the destructive action; the resume stack orders primary CTA → secondary → ghost, with Esc/close mapping to the safe fallback.
- Windows parity: when the native Windows shell is styled, map controls to these same roles, approximate dimensions, radii, colors, focus affordance, and emphasis order rather than platform-default gray buttons.

## Accessibility
- Target standard: practical keyboard and screen-reader friendly GTK controls.
- Keyboard/focus behavior: buttons and fields remain native controls; wizard navigation must not require pointer-only interaction.
- Contrast/readability: preserve existing light/dark overrides and accent contrast.
- Screen-reader semantics: use explicit button labels and concise section headings; avoid icon-only critical actions except delete buttons with tooltips.
- Reduced motion and sensory considerations: keep transitions short/native and do not rely on animation for state.

## Responsive behavior
- Supported breakpoints/devices: desktop windows around 1280×680 down to the existing 320px min launch scroller width.
- Layout adaptations: dashboard cards, saved board cards, and template grids should wrap via `FlowBox`; wizard content should scroll only inside the current step when needed; board columns remain horizontally scannable in normal desktop widths.
- Touch/hover differences: all critical actions are buttons with visible labels; hover only enhances affordance.

## Interaction states
- Loading: startup load warnings appear near the dashboard/wizard top; board tabs poll disk changes and refresh visible cards.
- Empty: no saved workspaces or saved boards shows encouraging empty copy and primary new-setup actions; empty board columns use compact "No tasks" states.
- Error: invalid workspace/project paths use existing `path-invalid` styling and log launch/open failures.
- Success: selected presets/templates receive selected-card styling; launch transitions the tab into the workspace runtime; opening a board transitions the tab into the Kanban runtime.
- Disabled: Back is disabled at step 1; Update Preset is hidden until editing an existing preset; board actions that need an existing task or agent config should fail visibly through board/log status rather than silently.
- Offline/slow network, if applicable: not applicable for local Core launch flow.

## Content voice
- Tone: concise, encouraging, operator-oriented.
- Terminology: use “workspace” for saved/openable terminal setups, “layout” for tile arrangement, “board” or “Kanban board” for project task boards, and “preset” where storage semantics matter.
- Microcopy rules: tell users what the next click does; prefer action verbs (“Open”, “Edit”, “Create”) over abstract labels.

## Implementation constraints
- Framework/styling system: Rust GTK4/libadwaita with existing CSS classes in `resources/style.css`.
- Design-token constraints: no new dependency or token framework; extend existing CSS classes only.
- Performance constraints: launch deck rebuilds should stay cheap; board polling should stay lightweight and based on board file mtime; avoid background services for local preset or board cards.
- Compatibility constraints: Linux GTK path is implemented here; Windows native shell must not be broken by shared model changes; the bundled MCP server must remain GTK-free and usable by external agent CLIs.
- Test/screenshot expectations: run Rust formatting/checks/tests; board/MCP changes should cover storage, service, MCP, and agent config tests; visual smoke is manual unless a screenshot harness exists.

## Open questions
- [ ] Should future workspace cards distinguish “preset template” from “last opened workspace session”? / owner: product / impact: naming and persistence model.
- [ ] Should invalid saved roots prompt with a folder chooser before opening? / owner: product / impact: faster recovery for moved projects.
- [ ] Should saved Kanban board cards show recent task counts without eagerly loading every project board? / owner: product/engineering / impact: launch dashboard performance and information density.
