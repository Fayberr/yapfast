# Theme port parity ledger

Last audited against the owner Slint implementation on 2026-11-19. A row is
**complete** only when the owner behavior reaches an iced consumer, its UI is
in the corresponding settings workflow, and a focused test covers it. Parsing
a token in `yapfast-core` does not make a row complete.

## Complete and tested

- Owner package discovery and `Package::resolve_with`; persisted selection,
  options, content permission, Performance and Reduce motion in `layout.json`.
- Base colours through the kit `Theme` conversion boundary, with Wired colours
  as fallbacks. Package surface gradients and all 30 `EXTRA_COLOURS` are
  retained in the companion registry and consumed across main panes, Settings,
  menus, pickers, embeds, code, labels, actions, composer/focus, selected/mentioned
  rows, profile card buttons, tooltips, scrims and badges.
- Shapes, radii, solid/dashed/dotted borders, shadows, all eight ordinary
  spacing tokens and the covered pane/list geometry.
- Popup/hover/press motion under Performance and Reduce motion.
- The separate Transitions between channels choice drives the resolved owner
  fade or dissolve transition over the retained previous chat surface.
- Ordinary layout modes, non-rotating list styling (including channel/member row
  and header fonts, weights, fill and offset shadows; member title shadows,
  rules and split count styling; rail label font), top/tree rails, grid and
  collapsed members, and content opt-in.
- Moving surface/list art, particles, fake glass, procedural overlays, icon and
  sound replacement (including Hover/Press interface cues with 70 ms hover gap),
  media recolouring and dithering across all non-deferred surfaces including
  Tooltip art/glass.
- Content opt-in following: `profiles` (`themes` and `effects`), illuminated
  avatar `initials` (messages and optional member rows with hover reveal),
  `embeds` (accent bar + accent-text link), and `activity` (accent LIVE pill
  and card/progress styling).
- Non-rotating event slots (`Row`, `Select`, `Hover`, `Click`, `NameHover`
  decrypt on people/message authors/members, bounce `Reaction` stamp without
  rotation), non-rotating declared `animations` (`Window` tint/opacity, `Accent`
  colour tick, `Home`, `Unread`, `Mention`, `Typing`, `ChannelName`,
  `ServerName`, `ProfileName`, `OwnAvatar`, phased `RailIcon`), and
  `motion.unfocused` / window minimize & settings-coverage `Pace` gating.
- A dedicated Settings → Themes destination; Appearance keeps quick choices
  and routes Browse/New/Edit into theme management.
- Settings → Themes exposes Installed (with preview thumbnails, Remix line,
  plain-language cost weight badge + expandable details, List vs 2-column
  Gallery toggle, and empty-state navigation actions), Marketplace (2-column
  gallery and empty-state actions), Add from URL, Create tabs and protected
  install review.
- Appearance → Readable text is persisted, restores stock reading faces across
  controls/panels, and disables hover decrypt effects.
- Installed packages support Apply/Turn off, 30-second Try/Keep/Revert,
  rollback and uninstall. Trials do not persist or install reviewed bytes
  until Keep, and invalid/deleted Back targets safely fall to stock.
- Package notification sounds, interface sounds and interface volume are
  separate per-theme choices and survive install/rollback refreshes.
- Auto dual-variant packages follow live iced system light/dark changes while
  explicit mode choices and the persisted package id remain unchanged.
- The owner emergency turn-off shortcut (`Ctrl+Shift+T`) works even while a
  settings layer or another overlay has keyboard focus.
- Existing owner custom themes preserve `creator` and `folder` data across an
  iced read/write round trip.
- The owner's non-rotating bubble gel and IRC rule draw around and beside
  virtual timeline content, with focused pixel tests.
- The owner guided creator has Start, Colours, Shape, Fonts and Advanced,
  newest-Pictures suggestions, picture blur/dim, built-in and imported fonts,
  exact asset-aware undo/start-over, package save/export and guarded deletion.
  Its checked draft resolves through a temporary companion, so colours,
  shapes, fonts, picture art and glass preview live and unregister on Cancel.

## Remaining edge-case differences (non-blocking / iced architectural boundaries)

- `letter_spacing` is retained in `TypeStyle` (`yapfast-core` / kit boundary),
  since iced 0.14's `text::Span` / `Paragraph` API does not expose per-run
  tracking.
- `RailTiles.{focus, focus_radius}` and `ChannelRows.{focus_outset, focus_radius}`
  use iced's `mouse_area` row/tile model rather than Slint per-row Tab
  `FocusScope` boxes; focus rings remain on controls, inputs and buttons.
- Secondary dialog/admin sub-screens (`bot_modal`, `server_settings`, `signin`,
  `mfa`, `channel_settings`) continue using stock helper text faces alongside
  themed kit `form`/`controls` primitives.

## Deliberately deferred by owner instruction

- Theme cursors.
- Rotation, card tilt/tape, magnification, reflected rails and ransom names.
- LCD previous-frame ghosting or any work requiring an iced fork.
