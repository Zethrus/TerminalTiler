# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-05-13
- Primary product surfaces: workspace launch deck, workspace creation/edit wizard, saved preset/workspace cards, tile editor, active workspace tabs.
- Evidence reviewed: `README.md`, `docs/core-boundary.md`, `src/ui/launch_screen.rs`, `src/ui/window.rs`, `src/ui/workspace_view.rs`, `src/model/preset.rs`, `src/model/layout.rs`, `resources/style.css`.

## Brand
- Personality: focused, command-center, native desktop, powerful but calm.
- Trust signals: local-first presets, explicit workspace paths, clear launch/edit actions, reversible navigation, visible validation.
- Avoid: one giant scroll form, hidden destructive actions, cloud/external-only assumptions in Core, decorative UI that obscures workflow setup.

## Product goals
- Goals: make creating a terminal workspace feel guided; make existing saved workspaces easy to reopen; make editing a saved workspace discoverable; preserve advanced tile customization without forcing users to confront every field at once.
- Non-goals: adding external synchronization, accounts, new dependencies, or a separate design-system framework.
- Success signals: users see saved workspaces or a prominent “New Workspace Layout” action first; each wizard screen has one clear decision; saved presets can be opened, copied, updated, and deleted locally.

## Personas and jobs
- Primary personas: developers, AI-agent operators, reviewers, release operators, Windows/WSL users needing repeatable terminal layouts.
- User jobs: reopen a known workspace quickly; create a new layout from a template; tune tile roles/commands/directories; save and later edit a preset.
- Key contexts of use: local desktop app launch, project directory setup, iterative workspace reconfiguration between tasks.

## Information architecture
- Primary navigation: app tab strip for workspace tabs; launch deck dashboard inside each empty workspace tab; wizard navigation for setup steps.
- Core routes/screens: saved workspace dashboard, new/edit workspace wizard, active workspace runtime, settings, assets manager.
- Content hierarchy: overview and quick actions first; path/name before template; appearance before tile detail; launch/save actions stay in the wizard footer.

## Design principles
- Principle 1: progressive disclosure — show one setup decision at a time and keep advanced tile fields in the final step.
- Principle 2: action clarity — “Open”, “Edit”, and “New Workspace Layout” should be explicit and visually distinct.
- Tradeoffs: the wizard adds clicks compared with a long form, but reduces scanning burden and makes saved workspaces easier to understand.

## Visual language
- Color: preserve the existing dark command-center palette with amber accent highlights and light-mode overrides.
- Typography: keep current GTK/libadwaita typography classes; use compact uppercase labels for step indicators and small meta copy.
- Spacing/layout rhythm: card-based surfaces with 16–18px panel padding; wizard body limited to the active step.
- Shape/radius/elevation: rounded cards, pill CTAs, selected-card borders, soft dark elevation as already encoded in `resources/style.css`.
- Motion: native GTK stack transitions are acceptable; avoid long or distracting animations.
- Imagery/iconography: use symbolic GTK icons only; no new bitmap assets.

## Components
- Existing components to reuse: `config-panel`, `preset-card`, `preset-card-compact`, `control-strip`, `pill-button`, `primary-cta-button`, `secondary-button`, `tile-editor-row`.
- New/changed components: launch dashboard, saved workspace action cards, wizard stepper, wizard footer navigation, step containers.
- Variants and states: selected template/preset, disabled Back on first step, primary Next vs Launch on final step, built-in preset “Save Copy” state, invalid path state.
- Token/component ownership: `resources/style.css` owns visual tokens/classes; `src/ui/launch_screen.rs` owns launch/wizard composition.

### Button role contract
- `primary-cta-button`: warm cream, highest-emphasis CTA for launch/open/create actions (`Launch Workspace`, `Open`, `New Workspace Layout`); compact enough for the footer but visually above every other action.
- `secondary-button`: dark glass support action for reversible navigation and editing (`Back`, `Update Preset`, `Edit`, `Workspaces`) with subtle amber border/highlight.
- `ghost-link-button`: low-emphasis transparent/dark action for exits such as `Close Tab`; readable without competing with the primary CTA.
- `destructive-button`: compact dark red-accent risk action for delete/close icon buttons; destructive actions should remain explicit and tooltip-backed when icon-only.
- `surface-button` / `surface-button-icon`: tile-editor and runtime surface controls; use the same dark glass token family at a smaller square/labeled size.
- Disabled buttons: muted but intentional role states, never generic washed-out platform gray.
- Windows parity: when the native Windows shell is styled, map owner-drawn/native buttons to these same roles, approximate dimensions, radii, colors, and emphasis order rather than default system gray.

## Accessibility
- Target standard: practical keyboard and screen-reader friendly GTK controls.
- Keyboard/focus behavior: buttons and fields remain native controls; wizard navigation must not require pointer-only interaction.
- Contrast/readability: preserve existing light/dark overrides and accent contrast.
- Screen-reader semantics: use explicit button labels and concise section headings; avoid icon-only critical actions except delete buttons with tooltips.
- Reduced motion and sensory considerations: keep transitions short/native and do not rely on animation for state.

## Responsive behavior
- Supported breakpoints/devices: desktop windows around 1280×680 down to the existing 320px min launch scroller width.
- Layout adaptations: dashboard cards and template grids should wrap via `FlowBox`; wizard content should scroll only inside the current step when needed.
- Touch/hover differences: all critical actions are buttons with visible labels; hover only enhances affordance.

## Interaction states
- Loading: startup load warnings appear near the dashboard/wizard top.
- Empty: no saved workspaces shows encouraging empty copy and a primary New Workspace Layout button.
- Error: invalid workspace paths use existing `path-invalid` styling and log launch failures.
- Success: selected presets/templates receive selected-card styling; launch transitions the tab into the workspace runtime.
- Disabled: Back is disabled at step 1; Update Preset is hidden until editing an existing preset.
- Offline/slow network, if applicable: not applicable for local Core launch flow.

## Content voice
- Tone: concise, encouraging, operator-oriented.
- Terminology: use “workspace” for saved/openable setups and “layout” for tile arrangement; keep “preset” where storage semantics matter.
- Microcopy rules: tell users what the next click does; prefer action verbs (“Open”, “Edit”, “Create”) over abstract labels.

## Implementation constraints
- Framework/styling system: Rust GTK4/libadwaita with existing CSS classes in `resources/style.css`.
- Design-token constraints: no new dependency or token framework; extend existing CSS classes only.
- Performance constraints: launch deck rebuilds should stay cheap; avoid background services for local preset cards.
- Compatibility constraints: Linux GTK path is implemented here; Windows native shell must not be broken by shared model changes.
- Test/screenshot expectations: run Rust formatting/checks/tests; visual smoke is manual unless a screenshot harness exists.

## Open questions
- [ ] Should future workspace cards distinguish “preset template” from “last opened workspace session”? / owner: product / impact: naming and persistence model.
- [ ] Should invalid saved roots prompt with a folder chooser before opening? / owner: product / impact: faster recovery for moved projects.
