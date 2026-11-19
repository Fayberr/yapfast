//! Appearance's theme shortcuts and profile switches, and the dedicated
//! Themes page: installed packages, the person's own themes, their options
//! and performance. The editor's changes show at once; Save keeps them and
//! Cancel puts the theme from before back. See docs/themes.md.

use iced::{
    Alignment, Border, Color, Element, Length,
    widget::{Space, button, column, container, row, stack, text},
};
use yapfast_core::{
    application::Action,
    theme::{self as palette, Custom},
    themes::{Mode, Performance, options},
};

use super::{Drop, Event as SettingsEvent, Page, Section, Settings, theme_marketplace};
use crate::{
    app::Message,
    controls::{Kind, action, lights, placed, slider},
    ctx::{Cx, Pref as Shown, Request},
    fonts::{BOLD, MEDIUM},
    form::{self, switch},
    pictures::Icon,
    themes::color,
};

/// A theme's tile: at least this wide, this tall, this far apart.
const TILE: f32 = 150.0;
const TILE_HEIGHT: f32 = 124.0;
const TILE_GAP: f32 = 12.0;
/// The editor's colours: two columns from this width, each row this tall.
const TWO_COLUMNS: f32 = 520.0;
const COLOUR_GAP: f32 = 24.0;
/// The name field.
const NAME: &str = "theme-name";

/// The theme editor while it's open: the theme as edited, each colour's
/// text and whether it's wrong, its name, and what it started from.
pub struct Editor {
    /// The custom theme edited, by id; none for a new one.
    pub editing: Option<String>,
    pub base: &'static str,
    pub theme: palette::Theme,
    pub name: String,
    /// Each colour's field, in `Theme::NAMES`' order: its name as
    /// `layout.json` has it, its text and whether it's a colour.
    pub fields: Vec<(String, String, bool)>,
    /// The theme a new one started as, among `palette::starts`.
    pub start: usize,
    /// Delete was pressed once; the next press deletes.
    pub confirming: bool,
}

impl Editor {
    fn new(
        editing: Option<String>,
        base: &'static str,
        theme: palette::Theme,
        name: String,
    ) -> Self {
        let mut editor = Self {
            editing,
            base,
            theme,
            name,
            fields: Vec::new(),
            start: 0,
            confirming: false,
        };
        editor.fill();
        editor
    }

    /// Every field's text from the theme's colours.
    fn fill(&mut self) {
        self.fields = palette::Theme::NAMES
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                Some((
                    palette::token(name),
                    palette::hex(self.theme.colour(index)?),
                    false,
                ))
            })
            .collect();
    }

    /// Takes a colour's text as typed: the theme takes it if it's a colour.
    /// Returns whether the theme changed.
    fn edit(&mut self, index: usize, text: String) -> bool {
        let parsed = palette::parse(&text);
        let Some(field) = self.fields.get_mut(index) else {
            return false;
        };
        (field.1, field.2) = (text, parsed.is_none());
        match (parsed, self.theme.colour_mut(index)) {
            (Some(colour), Some(slot)) if *slot != colour => {
                *slot = colour;
                true
            }
            _ => false,
        }
    }

    fn valid(&self) -> bool {
        self.fields.iter().all(|(_, _, wrong)| !wrong)
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Choose(String),
    TurnOff,
    Revert,
    Performance(i32),
    ReduceMotion(bool),
    ReadableText(bool),
    ThemeOption(String, String, serde_json::Value),
    ThemeContent(String, bool),
    ThemeNotificationSounds(String, bool),
    ThemeInterfaceSounds(String, bool),
    ThemeInterfaceVolume(String, u8),
    ThemeTransitions(bool),
    Creator(super::theme_creator::Event),
    New,
    Edit(String),
    Start(usize),
    Colour(usize, String),
    Dark(bool),
    Name(String),
    Save,
    Cancel,
    Delete,
    Profile(Cosmetic, bool),
}

/// Appearance's profile switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cosmetic {
    Effects,
    Decorations,
    Nameplates,
    Moving,
    NameStyles,
    ServerTags,
    Frames,
}

fn theme_message(event: Event) -> Message {
    crate::app::settings(SettingsEvent::Theme(event))
}

fn marketplace_message(event: theme_marketplace::Event) -> Message {
    crate::app::settings(SettingsEvent::Marketplace(event))
}

fn number(value: f64) -> serde_json::Value {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

fn modes_text(modes: &[Mode]) -> &'static str {
    match modes {
        [Mode::Dark, Mode::Light] => "Dark and light",
        [Mode::Light] => "Light",
        _ => "Dark",
    }
}

fn option_slider<'a>(
    theme: &crate::theme::Theme,
    option: &options::ThemeOption,
    value: &serde_json::Value,
    min: f64,
    max: f64,
    step: f64,
    message: impl Fn(serde_json::Value) -> Message + 'a,
) -> Element<'a, Message> {
    let steps = ((max - min) / step).round().max(1.0) as u32;
    let chosen = value.as_f64().unwrap_or(min).clamp(min, max);
    let at = ((chosen - min) / step).round().clamp(0.0, f64::from(steps)) as u32;
    let mut control = column![
        slider(option.label.clone(), at, steps, move |at| {
            message(number(min + f64::from(at) * step))
        })
        .step(1)
        .shown(option.describe(value))
        .mark(false)
        .view(theme)
    ]
    .spacing(4);
    if !option.description.is_empty() {
        control = control.push(form::note(theme, option.description.clone()));
    }
    control.into()
}

impl Settings {
    pub(super) fn theme_event(&mut self, event: Event, cx: &mut Cx<'_>) -> iced::Task<Message> {
        match event {
            Event::Creator(event) => return self.creator_event(event, cx),
            Event::Choose(id) => choose(cx, &id),
            Event::TurnOff => {
                cx.themes.turn_off(cx.client.root());
                cx.request(Request::Changed(Shown::Theme));
            }
            Event::Revert => {
                if cx.themes.revert(cx.client.root()) {
                    cx.request(Request::Changed(Shown::Theme));
                }
            }
            Event::Performance(index) => {
                let performance = Performance::from_index(index);
                cx.layout.theme_performance = performance;
                cx.keep("theme_performance", performance);
                cx.themes
                    .motion_changed(performance, cx.layout.reduce_motion);
                cx.request(Request::Changed(Shown::Theme));
            }
            Event::ReduceMotion(on) => {
                cx.layout.reduce_motion = on;
                cx.keep("reduce_motion", on);
                cx.themes.motion_changed(cx.layout.theme_performance, on);
                cx.request(Request::Changed(Shown::Theme));
            }
            Event::ReadableText(on) => {
                cx.layout.readable_text = on;
                cx.keep("readable_text", on);
                cx.themes.readable_changed(on);
                cx.request(Request::Changed(Shown::Theme));
            }
            Event::ThemeOption(id, option, value) => {
                let Some(choices) = cx.themes.set_option(
                    &id,
                    &option,
                    value,
                    cx.layout.theme_performance,
                    cx.layout.reduce_motion,
                ) else {
                    return iced::Task::none();
                };
                if choices.is_empty() {
                    cx.layout.theme_options.remove(&id);
                } else {
                    cx.layout.theme_options.insert(id, choices);
                }
                cx.keep("theme_options", &cx.layout.theme_options);
                cx.request(Request::Changed(Shown::Theme));
            }
            Event::ThemeContent(id, on) => {
                if on {
                    cx.layout.theme_content.insert(id);
                } else {
                    cx.layout.theme_content.remove(&id);
                }
                cx.keep("theme_content", &cx.layout.theme_content);
                cx.request(Request::Changed(Shown::Theme));
            }
            Event::ThemeNotificationSounds(id, on) => {
                change_sound_prefs(cx, &id, |prefs| prefs.on = on);
            }
            Event::ThemeInterfaceSounds(id, on) => {
                change_sound_prefs(cx, &id, |prefs| prefs.interface = on);
            }
            Event::ThemeInterfaceVolume(id, volume) => {
                change_sound_prefs(cx, &id, |prefs| prefs.volume = volume.min(100));
            }
            Event::ThemeTransitions(on) => {
                cx.layout.theme_transitions = on;
                cx.keep("theme_transitions", on);
            }
            Event::New => self.new_theme(cx),
            Event::Edit(id) => {
                if self.edit_creator(&id, cx) {
                    return iced::Task::none();
                }
                let Some(custom) = cx.themes.custom.iter().find(|custom| custom.id == id) else {
                    return iced::Task::none();
                };
                let editor = Editor::new(
                    Some(custom.id.clone()),
                    custom.base,
                    custom.theme,
                    custom.name.clone(),
                );
                self.open_editor(editor, cx);
            }
            Event::Start(index) => {
                let starts: Vec<_> = palette::starts(&cx.themes.custom)
                    .map(|start| (start.base, start.theme, copy_name(start.name)))
                    .collect();
                let Some(editor) = self.editor.as_mut() else {
                    return iced::Task::none();
                };
                let Some((base, theme, name)) = starts.get(index).cloned() else {
                    return iced::Task::none();
                };
                // It takes the new start's name too, unless the person
                // named it.
                if starts
                    .get(editor.start)
                    .is_some_and(|before| before.2 == editor.name)
                {
                    editor.name = name;
                }
                editor.base = base;
                editor.theme = theme;
                editor.start = index;
                editor.fill();
                self.open = None;
                self.show_edited(cx);
            }
            Event::Colour(index, text) => {
                let changed = self
                    .editor
                    .as_mut()
                    .is_some_and(|editor| editor.edit(index, text));
                if changed {
                    self.show_edited(cx);
                }
            }
            Event::Dark(dark) => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.theme.dark = dark;
                }
                self.show_edited(cx);
            }
            Event::Name(name) => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.name = name;
                }
            }
            Event::Save => self.save_theme(cx),
            Event::Cancel => self.cancel_theme(cx),
            Event::Delete => self.delete_theme(cx),
            Event::Profile(cosmetic, on) => self::cosmetic(cx, cosmetic, on),
        }
        iced::Task::none()
    }

    /// Opens the editor on a copy of the theme shown, which can start from
    /// any other.
    fn new_theme(&mut self, cx: &mut Cx<'_>) {
        let current = &cx.theme.id;
        let starts: Vec<_> = palette::starts(&cx.themes.custom)
            .map(|start| {
                (
                    start.id.to_owned(),
                    start.base,
                    start.theme,
                    copy_name(start.name),
                )
            })
            .collect();
        let start = starts
            .iter()
            .position(|(id, ..)| id == current)
            .unwrap_or(0);
        let (_, base, theme, name) = starts[start].clone();
        let mut editor = Editor::new(None, base, theme, name);
        editor.start = start;
        self.open_editor(editor, cx);
    }

    fn open_editor(&mut self, editor: Editor, cx: &mut Cx<'_>) {
        // As in the owner UI, New theme and Edit in Appearance lead to the
        // creator inside Settings → Themes rather than turning Appearance
        // into a second theme-management page.
        self.section = Some(Section::Themes);
        self.editor = Some(editor);
        self.show_edited(cx);
    }

    /// The window in the theme as edited, without keeping it.
    fn show_edited(&mut self, cx: &mut Cx<'_>) {
        let Some(editor) = &self.editor else {
            return;
        };
        let id = editor.editing.as_deref().unwrap_or("editing");
        self.preview = Some(crate::themes::theme(id, &editor.theme));
        cx.request(Request::Changed(Shown::Theme));
    }

    /// Closes the editor: the window shows the theme chosen again.
    pub(super) fn close_editor(&mut self, cx: &mut Cx<'_>) -> Option<Editor> {
        let editor = self.editor.take()?;
        if self.preview.take().is_some() {
            cx.request(Request::Changed(Shown::Theme));
        }
        Some(editor)
    }

    /// Keeps the theme as edited among the person's own, and chooses it.
    fn save_theme(&mut self, cx: &mut Cx<'_>) {
        if !self.editor.as_ref().is_some_and(Editor::valid) {
            return;
        }
        let Some(editor) = self.close_editor(cx) else {
            return;
        };
        let name = match editor.name.trim() {
            "" => "My theme".to_owned(),
            name => name.to_owned(),
        };
        let custom = &mut cx.themes.custom;
        let saved = editor
            .editing
            .as_ref()
            .and_then(|id| custom.iter_mut().find(|custom| custom.id == *id));
        let id = match saved {
            Some(saved) => {
                saved.name = name;
                saved.theme = editor.theme;
                saved.id.clone()
            }
            None => {
                let id = palette::new_id(custom);
                custom.push(Custom {
                    id: id.clone(),
                    name,
                    base: editor.base,
                    theme: editor.theme,
                    creator: None,
                    folder: None,
                });
                id
            }
        };
        cx.keep("custom_themes", &cx.themes.custom);
        choose(cx, &id);
    }

    /// Closes the editor and shows the theme from before it opened.
    pub(super) fn cancel_theme(&mut self, cx: &mut Cx<'_>) {
        self.close_editor(cx);
        self.close_creator(cx);
    }

    /// Deletes the theme edited; if it was the chosen one, Yap shows.
    fn delete_theme(&mut self, cx: &mut Cx<'_>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        if !editor.confirming {
            editor.confirming = true;
            return;
        }
        let Some(editor) = self.close_editor(cx) else {
            return;
        };
        if let Some(id) = editor.editing {
            let changed = cx.themes.forget_custom(cx.client.root(), &id);
            cx.layout.custom_themes.clone_from(&cx.themes.custom);
            cx.keep("custom_themes", &cx.themes.custom);
            if changed {
                cx.request(Request::Changed(Shown::Theme));
            }
        }
    }
}

/// Chooses the theme `id` and keeps the choice, if there's such a theme:
/// the window shows it.
fn choose(cx: &mut Cx<'_>, id: &str) {
    let Some(theme) = cx.themes.find(id) else {
        return;
    };
    cx.themes.choose(cx.client.root(), &theme.id);
    cx.request(Request::ThemeApplied);
}

/// Changes one applied package's sound choices, omitting the map entry when
/// every value is the owner's default. Notification replacements are
/// reapplied immediately when this is the active theme.
fn change_sound_prefs(
    cx: &mut Cx<'_>,
    id: &str,
    change: impl FnOnce(&mut crate::themes::SoundPrefs),
) {
    let Some(mut prefs) = cx.themes.sound_prefs(id) else {
        return;
    };
    change(&mut prefs);
    if !cx.themes.set_sound_prefs(id, prefs) {
        return;
    }
    if prefs == crate::themes::SoundPrefs::default() {
        cx.layout.theme_sounds.remove(id);
    } else {
        cx.layout.theme_sounds.insert(id.to_owned(), prefs);
    }
    cx.keep("theme_sounds", &cx.layout.theme_sounds);
    let active = cx.themes.chosen_id().to_owned();
    if active == id {
        cx.themes.apply_sounds(&active);
    }
}

/// A profile switch: kept, and the core told what may now show.
fn cosmetic(cx: &mut Cx<'_>, cosmetic: Cosmetic, on: bool) {
    let prefs = &mut cx.layout.prefs;
    let (slot, key) = match cosmetic {
        Cosmetic::Effects => (&mut prefs.profile_effects, "profile_effects"),
        Cosmetic::Decorations => (&mut prefs.decorations, "decorations"),
        Cosmetic::Nameplates => (&mut prefs.nameplates, "nameplates"),
        Cosmetic::Moving => (&mut prefs.moving_cosmetics, "moving_cosmetics"),
        Cosmetic::NameStyles => (&mut prefs.name_styles, "name_styles"),
        Cosmetic::ServerTags => (&mut prefs.server_tags, "server_tags"),
        Cosmetic::Frames => (&mut prefs.profile_frames, "profile_frames"),
    };
    *slot = Some(on);
    cx.keep(key, on);
    if cosmetic == Cosmetic::Nameplates {
        cx.dispatch(Action::HideNameplates(!on));
    } else {
        let cosmetics = cx.layout.prefs.cosmetics();
        cx.dispatch(Action::SetCosmetics(cosmetics));
    }
    cx.request(Request::Changed(Shown::Roster));
}

impl<'a> Page<'a> {
    pub(super) fn appearance_page(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let tiles = self.theme_tiles(true);
        let prefs = &self.cx.layout.prefs;
        let cosmetics = prefs.cosmetics();
        let profile = |label, description, on: bool, which| {
            switch(label, on, Some(theme_message(Event::Profile(which, !on))))
                .description(description)
                .view(self.cx.look)
        };
        let theme_options = self.installed_theme_options();
        let installed_name = self
            .cx
            .themes
            .installed
            .iter()
            .find(|installed| self.cx.themes.chosen_id() == installed.id)
            .map(|installed| installed.name.clone());
        let actions = row![
            action(
                theme,
                "Browse themes",
                Kind::Plain,
                Some(crate::app::settings(SettingsEvent::Open(Section::Themes))),
            )
            .small()
            .icon(
                self.cx
                    .look
                    .icon(Icon::Brush, 16, crate::theme::rgb(theme.text))
            ),
            action(
                theme,
                "New theme",
                Kind::Plain,
                Some(theme_message(Event::New))
            )
            .small()
            .icon(
                self.cx
                    .look
                    .icon(Icon::Plus, 16, crate::theme::rgb(theme.text))
            ),
        ]
        .spacing(8);
        let installed_note: Element<'a, Message> = installed_name.map_or_else(
            || Space::new().height(0).into(),
            |name| {
                form::note(
                    theme,
                    format!(
                        "Showing {name}, an installed theme. Choose one above, or manage it in Themes."
                    ),
                )
            },
        );
        form::page([
            form::title(theme, "Appearance"),
            form::group([
                form::beside(form::heading(theme, "THEME"), actions),
                tiles,
            ]),
            installed_note,
            theme_options,
            switch(
                "Readable text",
                self.cx.layout.readable_text,
                Some(theme_message(Event::ReadableText(
                    !self.cx.layout.readable_text,
                ))),
            )
            .description(
                "Messages, names, channels and what you type use Yapfast's own font in every theme. The theme's art, colours and layout stay.",
            )
            .view(self.cx.look),
            self.title_bar_switch(),
            column![
                form::heading(theme, "PROFILES"),
                profile(
                    "Play profile effects",
                    "The animations some people put over their profile cards.",
                    cosmetics.effects,
                    Cosmetic::Effects,
                ),
                profile(
                    "Show avatar decorations",
                    "The pictures around people's avatars.",
                    cosmetics.decorations,
                    Cosmetic::Decorations,
                ),
                profile(
                    "Show nameplates",
                    "The pictures Discord members equip behind their rows in the member list and on your account card.",
                    prefs.nameplates.unwrap_or(true),
                    Cosmetic::Nameplates,
                ),
                profile(
                    "Animate decorations and nameplates",
                    "They play when pointed at, as in Discord. Off, they hold still.",
                    cosmetics.moving,
                    Cosmetic::Moving,
                ),
                profile(
                    "Show name styles",
                    "The fonts and colours people give their display names.",
                    cosmetics.name_styles,
                    Cosmetic::NameStyles,
                ),
                profile(
                    "Show server tags",
                    "The tag and its badge beside people's names.",
                    cosmetics.server_tags,
                    Cosmetic::ServerTags,
                ),
                profile(
                    "Show profile frames",
                    "The frames some people put around their profile cards.",
                    cosmetics.frames,
                    Cosmetic::Frames,
                ),
            ]
            .spacing(12)
            .into(),
        ])
    }

    /// Settings → Themes, corresponding to the owner UI's Installed tab.
    /// Marketplace, URL installation and the creator's advanced workflow
    /// remain separate later slabs; package management does not live in
    /// Appearance merely because the iced rewrite originally lacked this
    /// destination.
    pub(super) fn themes_page(self) -> Element<'a, Message> {
        let settings = self.s;
        let theme = self.cx.look.theme;
        if let Some(creator) = &settings.creator {
            return self.creator_view(creator);
        }
        if let Some(editor) = &settings.editor {
            return self.editor_view(editor, settings.open == Some(Drop::Start));
        }

        let tabs = self.theme_tabs();
        if settings.marketplace.tab != theme_marketplace::Tab::Installed {
            return form::page([
                tabs,
                self.theme_trial_banner(),
                settings
                    .marketplace
                    .view(self.cx.look, self.cx.themes)
                    .map(marketplace_message),
            ]);
        }

        let custom_applied = self
            .cx
            .themes
            .custom
            .iter()
            .any(|custom| self.cx.themes.chosen_id() == custom.id);
        let mut own_actions = row![].spacing(8);
        if custom_applied {
            own_actions = own_actions.push(
                action(
                    theme,
                    "Turn off theme",
                    Kind::Plain,
                    Some(theme_message(Event::TurnOff)),
                )
                .small(),
            );
        }
        own_actions = own_actions.push(
            action(
                theme,
                "Create theme",
                Kind::Primary,
                Some(theme_message(Event::Creator(
                    super::theme_creator::Event::Open,
                ))),
            )
            .small(),
        );
        own_actions = own_actions.push(
            action(
                theme,
                "Colour editor",
                Kind::Plain,
                Some(theme_message(Event::New)),
            )
            .small()
            .icon(
                self.cx
                    .look
                    .icon(Icon::Plus, 16, crate::theme::rgb(theme.text)),
            ),
        );
        let own: Element<'a, Message> = if self.cx.themes.custom.is_empty() {
            form::note(
                theme,
                "No themes of your own yet. Create theme opens the guided creator; Colour editor keeps the earlier palette-only workflow.",
            )
        } else {
            self.theme_tiles(false)
        };

        let previous = self.cx.themes.previous().map(|(_, name)| {
            action(
                theme,
                format!("Back to {name}"),
                Kind::Plain,
                Some(theme_message(Event::Revert)),
            )
            .small()
            .icon(self.cx.look.icon(
                Icon::ArrowLeft,
                16,
                crate::theme::rgb(theme.text),
            ))
        });

        form::page([
            tabs,
            self.theme_trial_banner(),
            form::title(theme, "Themes"),
            form::note(
                theme,
                "Themes restyle Yapfast: its colours, fonts, shapes, art and sounds. A theme is data, never code.",
            ),
            previous.map_or_else(
                || Space::new().height(0).into(),
                |previous| row![previous].into(),
            ),
            self.installed_themes(),
            self.refused_themes(),
            form::group([
                form::beside(form::heading(theme, "YOUR THEMES"), own_actions),
                own,
            ]),
            self.installed_theme_options(),
            self.installed_theme_content(),
            self.installed_theme_sounds(),
            self.theme_performance(),
        ])
    }

    fn theme_tabs(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let current = self.s.marketplace.tab;
        let tab = |label, wanted| {
            action(
                theme,
                label,
                if current == wanted {
                    Kind::Primary
                } else {
                    Kind::Plain
                },
                (current != wanted)
                    .then_some(marketplace_message(theme_marketplace::Event::Tab(wanted))),
            )
            .small()
        };
        row![
            tab("Installed", theme_marketplace::Tab::Installed),
            tab("Marketplace", theme_marketplace::Tab::Marketplace),
            tab("Add from URL", theme_marketplace::Tab::AddFromUrl),
            action(
                theme,
                "Create",
                Kind::Plain,
                Some(theme_message(Event::Creator(
                    super::theme_creator::Event::Open,
                ))),
            )
            .small(),
        ]
        .spacing(8)
        .into()
    }

    fn theme_trial_banner(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let Some(trial) = self.cx.themes.trial() else {
            return Space::new().height(0).into();
        };
        container(
            row![
                column![
                    text(format!("Trying {}", trial.name))
                        .size(15)
                        .font(BOLD)
                        .color(theme.text),
                    text(format!("Reverting in {} seconds", trial.seconds))
                        .size(13)
                        .color(theme.secondary),
                ]
                .spacing(2)
                .width(Length::Fill),
                action(
                    theme,
                    "Revert",
                    Kind::Plain,
                    Some(marketplace_message(theme_marketplace::Event::RevertTrial)),
                )
                .small(),
                action(
                    theme,
                    "Keep",
                    Kind::Primary,
                    Some(marketplace_message(theme_marketplace::Event::KeepTrial)),
                )
                .small(),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(12)
        .style(move |_| crate::style::rounded(theme.raised, 8))
        .into()
    }

    /// Installed package themes, with the applied one offering the way
    /// back to the stock default.
    fn installed_themes(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let gallery = self.s.marketplace.gallery;
        let header: Element<'a, Message> = if self.cx.themes.installed.is_empty() {
            form::heading(theme, "INSTALLED THEMES")
        } else {
            let toggle = |label: &'static str, wanted: bool| {
                action(
                    theme,
                    label,
                    if gallery == wanted {
                        Kind::Primary
                    } else {
                        Kind::Plain
                    },
                    (gallery != wanted).then_some(marketplace_message(
                        theme_marketplace::Event::ToggleGallery(wanted),
                    )),
                )
                .small()
            };
            form::beside(
                form::heading(theme, "INSTALLED THEMES"),
                row![toggle("List", false), toggle("Gallery", true)].spacing(6),
            )
        };
        let mut rows = column![].spacing(10);
        if self.cx.themes.installed.is_empty() && self.cx.themes.refused.is_empty() {
            rows = rows.push(
                container(
                    column![
                        form::note(theme, "No theme packages are installed in this data folder."),
                        form::note(
                            theme,
                            "Find one in the Marketplace, add one from its GitHub repository, or make your own in Create.",
                        ),
                        row![
                            action(
                                theme,
                                "Browse the Marketplace",
                                Kind::Primary,
                                Some(marketplace_message(theme_marketplace::Event::Tab(
                                    theme_marketplace::Tab::Marketplace,
                                ))),
                            )
                            .small(),
                            action(
                                theme,
                                "Add from URL",
                                Kind::Plain,
                                Some(marketplace_message(theme_marketplace::Event::Tab(
                                    theme_marketplace::Tab::AddFromUrl,
                                ))),
                            )
                            .small(),
                            action(
                                theme,
                                "Create",
                                Kind::Plain,
                                Some(theme_message(Event::Creator(
                                    super::theme_creator::Event::Open,
                                ))),
                            )
                            .small(),
                        ]
                        .spacing(8),
                    ]
                    .spacing(8),
                )
                .padding(14)
                .width(Length::Fill)
                .style(move |_| crate::style::rounded(theme.raised, 8)),
            );
        } else if self.cx.themes.installed.is_empty() {
            rows = rows.push(form::note(theme, "No installed theme could be opened."));
        } else if gallery {
            let mut grid = column![].spacing(12);
            for chunk in self.cx.themes.installed.chunks(2) {
                let mut line = row![].spacing(12);
                for installed in chunk {
                    line = line.push(
                        container(self.installed_card(installed, true))
                            .width(Length::FillPortion(1)),
                    );
                }
                if chunk.len() == 1 {
                    line = line.push(Space::new().width(Length::FillPortion(1)));
                }
                grid = grid.push(line);
            }
            rows = rows.push(grid);
        } else {
            for installed in &self.cx.themes.installed {
                rows = rows.push(self.installed_card(installed, false));
            }
        }
        form::group([header, rows.into()])
    }

    fn installed_card(
        self,
        installed: &'a crate::themes::Installed,
        gallery: bool,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let applied = self.cx.themes.chosen_id() == installed.id;
        let authors = installed.authors.join(", ");
        let details = if authors.is_empty() {
            format!("v{} · {}", installed.version, modes_text(&installed.modes))
        } else {
            format!(
                "by {authors} · v{} · {}",
                installed.version,
                modes_text(&installed.modes)
            )
        };
        let thumb: Element<'a, Message> = if let Some(handle) = &installed.preview {
            let pic = iced::widget::image(handle.clone())
                .content_fit(iced::ContentFit::Cover)
                .height(if gallery { 110.0 } else { 78.0 });
            if gallery {
                pic.width(Length::Fill).into()
            } else {
                pic.width(124.0).into()
            }
        } else {
            preview(
                &installed.palette_preview,
                if gallery { 220.0 } else { 124.0 },
            )
        };
        let mut label = column![
            row![
                text(installed.name.clone())
                    .size(theme.type_size("names", 15.0))
                    .font(theme.font("names", if applied { BOLD } else { MEDIUM }))
                    .color(theme.text)
                    .width(Length::Fill),
                text(if applied { "Applied" } else { "Installed" })
                    .size(theme.type_size("small", 12.0))
                    .font(theme.font("small", MEDIUM))
                    .color(if applied {
                        theme.accent_text()
                    } else {
                        theme.muted
                    }),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(details)
                .size(theme.type_size("small", 13.0))
                .font(theme.font("small", crate::fonts::REGULAR))
                .color(theme.muted),
        ]
        .spacing(3)
        .width(Length::Fill);
        if let Some(based_on) = &installed.based_on {
            label = label.push(
                text(format!("Remixes {based_on}"))
                    .size(theme.type_size("small", 12.0))
                    .font(theme.font("small", crate::fonts::REGULAR))
                    .color(theme.muted),
            );
        }
        if !installed.description.is_empty() {
            label = label.push(
                text(installed.description.clone())
                    .size(theme.type_size("body", 13.0))
                    .font(theme.font("body", crate::fonts::REGULAR))
                    .color(theme.secondary),
            );
        }
        let cost = theme_marketplace::cost_line(
            theme,
            &installed.cost,
            &[],
            self.s.marketplace.cost_details.contains(&installed.id),
            marketplace_message(theme_marketplace::Event::ToggleCostDetails(
                installed.id.clone(),
            )),
        );
        label = label.push(cost);
        if !installed.notes.is_empty() {
            label = label.push(
                text(format!(
                    "Left out by this Yapfast: {}",
                    installed.notes.join("; ")
                ))
                .size(theme.type_size("small", 12.0))
                .font(theme.font("small", crate::fonts::REGULAR))
                .color(theme.muted),
            );
        }
        let press = if applied {
            Event::TurnOff
        } else {
            Event::Choose(installed.id.clone())
        };
        let package_id = installed
            .id
            .strip_prefix(crate::themes::INSTALLED)
            .unwrap_or(&installed.id)
            .to_owned();
        let mut actions = row![].spacing(6).align_y(Alignment::Center);
        if !applied {
            actions = actions.push(
                action(
                    theme,
                    "Try",
                    Kind::Plain,
                    Some(marketplace_message(theme_marketplace::Event::TryInstalled(
                        installed.id.clone(),
                    ))),
                )
                .small(),
            );
        }
        actions = actions.push(
            action(
                theme,
                if applied { "Turn off theme" } else { "Apply" },
                if applied { Kind::Plain } else { Kind::Primary },
                Some(theme_message(press)),
            )
            .small(),
        );
        if installed.previous.is_some() {
            actions = actions.push(
                action(
                    theme,
                    "Roll back",
                    Kind::Plain,
                    Some(marketplace_message(theme_marketplace::Event::RollBack(
                        package_id.clone(),
                    ))),
                )
                .small(),
            );
        }
        actions = actions.push(
            action(
                theme,
                "Remove",
                Kind::Ghost,
                Some(marketplace_message(theme_marketplace::Event::Uninstall(
                    package_id,
                ))),
            )
            .small(),
        );
        if gallery {
            container(column![thumb, label, actions].spacing(8))
                .padding(12)
                .width(Length::Fill)
                .style(move |_| crate::style::rounded(theme.raised, 8))
                .into()
        } else {
            container(
                row![thumb, label, actions]
                    .spacing(12)
                    .align_y(Alignment::Center),
            )
            .padding(10)
            .width(Length::Fill)
            .style(move |_| crate::style::rounded(theme.raised, 8))
            .into()
        }
    }

    /// Lock entries or packages the core refused, kept visible with the
    /// exact reason returned by the installed-theme reader.
    fn refused_themes(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        if self.cx.themes.refused.is_empty() {
            return Space::new().height(0).into();
        }
        let mut rows = column![].spacing(10);
        for refused in &self.cx.themes.refused {
            rows = rows.push(
                column![
                    text(refused.id.clone())
                        .size(14)
                        .font(MEDIUM)
                        .color(theme.danger_text),
                    form::note_in(refused.reason.clone(), theme.danger_text),
                ]
                .spacing(3),
            );
        }
        form::group([
            form::heading(theme, "THEMES THAT COULDN'T BE OPENED"),
            form::note(
                theme,
                "These entries remain installed, but Yapfast will not use files it cannot verify.",
            ),
            rows.into(),
        ])
    }

    /// The applied package's switches, choices and sliders. Values are
    /// normalized by the core option before the package is resolved again.
    fn installed_theme_options(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let Some(installed) = self
            .cx
            .themes
            .installed
            .iter()
            .find(|installed| self.cx.themes.chosen_id() == installed.id)
        else {
            return Space::new().height(0).into();
        };
        let mut rows = column![].spacing(12);
        let mut any = false;
        for option in &installed.options {
            if !option.active(&installed.options, &installed.choices) {
                continue;
            }
            any = true;
            let value = option.value(&installed.choices);
            let id = installed.id.clone();
            let option_id = option.id.clone();
            let message = move |value| {
                theme_message(Event::ThemeOption(id.clone(), option_id.clone(), value))
            };
            let control: Element<'a, Message> = match &option.kind {
                options::Kind::Toggle { .. } => switch(
                    option.label.clone(),
                    value.as_bool().unwrap_or(false),
                    Some(message(serde_json::Value::Bool(
                        !value.as_bool().unwrap_or(false),
                    ))),
                )
                .description(option.description.clone())
                .view(self.cx.look),
                options::Kind::Choice { choices, .. } => {
                    let selected = value
                        .as_str()
                        .and_then(|value| choices.iter().position(|choice| choice.value == value));
                    let offered = choices
                        .iter()
                        .map(|choice| form::Choice {
                            title: choice.label.clone().into(),
                            description: String::new().into(),
                            colour: None,
                            offered: true,
                        })
                        .collect();
                    let id = installed.id.clone();
                    let option_id = option.id.clone();
                    let values: Vec<_> =
                        choices.iter().map(|choice| choice.value.clone()).collect();
                    column![
                        text(option.label.clone())
                            .size(14)
                            .font(MEDIUM)
                            .color(theme.text),
                        if option.description.is_empty() {
                            Space::new().height(0).into()
                        } else {
                            form::note(theme, option.description.clone())
                        },
                        form::options(theme, offered, selected, true, move |index| {
                            theme_message(Event::ThemeOption(
                                id.clone(),
                                option_id.clone(),
                                serde_json::Value::String(values[index].clone()),
                            ))
                        })
                    ]
                    .spacing(6)
                    .into()
                }
                options::Kind::Slider { min, max, step, .. } => {
                    option_slider(theme, option, &value, *min, *max, *step, message)
                }
                options::Kind::Hue { .. } => {
                    option_slider(theme, option, &value, 0.0, 360.0, 1.0, message)
                }
            };
            rows = rows.push(control);
        }
        if !any {
            return Space::new().height(0).into();
        }
        form::group([form::heading(theme, "THEME OPTIONS"), rows.into()])
    }

    /// People's content is the one package surface that stays opt-in, as
    /// the owner theme system promises. The switch is offered only when
    /// the applied package actually changes content.
    fn installed_theme_content(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let id = self.cx.themes.chosen_id();
        let Some(phrase) = self.cx.themes.content_phrase(id) else {
            return Space::new().height(0).into();
        };
        let on = self.cx.layout.theme_content.contains(id);
        form::group([
            form::heading(theme, "THEME CONTENT"),
            switch(
                "Let this theme style people's content",
                on,
                Some(theme_message(Event::ThemeContent(id.to_owned(), !on))),
            )
            .description(format!("When allowed: {phrase}."))
            .view(self.cx.look),
        ])
    }

    /// Sound replacements the applied package actually contains. Their
    /// permissions and interface volume belong to that package id.
    fn installed_theme_sounds(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let id = self.cx.themes.chosen_id();
        let Some((notifications, interface)) = self.cx.themes.has_sounds(id) else {
            return Space::new().height(0).into();
        };
        if !notifications && !interface {
            return Space::new().height(0).into();
        }
        let prefs = self.cx.themes.sound_prefs(id).unwrap_or_default();
        let mut rows = column![].spacing(10);
        if notifications {
            rows = rows.push(
                switch(
                    "Use theme notification sounds",
                    prefs.on,
                    Some(theme_message(Event::ThemeNotificationSounds(
                        id.to_owned(),
                        !prefs.on,
                    ))),
                )
                .description(
                    "Use this theme's sounds for messages, mentions, calls and other notifications.",
                )
                .view(self.cx.look),
            );
        }
        if interface {
            rows = rows.push(
                switch(
                    "Use theme interface sounds",
                    prefs.interface,
                    Some(theme_message(Event::ThemeInterfaceSounds(
                        id.to_owned(),
                        !prefs.interface,
                    ))),
                )
                .description("Play this theme's sounds for buttons, menus and other actions.")
                .view(self.cx.look),
            );
            rows = rows.push(
                slider("Interface sound volume", u32::from(prefs.volume), 100, {
                    let id = id.to_owned();
                    move |volume| {
                        theme_message(Event::ThemeInterfaceVolume(id.clone(), volume as u8))
                    }
                })
                .step(1)
                .shown(format!("{}%", prefs.volume))
                .mark(false)
                .view(theme),
            );
        }
        form::group([form::heading(theme, "THEME SOUNDS"), rows.into()])
    }

    /// The package-wide performance and reduced-motion settings.
    fn theme_performance(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let choices = vec![
            form::Choice {
                title: "Full".into(),
                description: "Run every theme animation at its requested rate.".into(),
                colour: None,
                offered: true,
            },
            form::Choice {
                title: "Balanced".into(),
                description: "Keep heavy theme animation still and limit the rest.".into(),
                colour: None,
                offered: true,
            },
            form::Choice {
                title: "Light".into(),
                description: "Use colours, fonts, shapes and spacing without theme animation."
                    .into(),
                colour: None,
                offered: true,
            },
        ];
        form::group([
            form::heading(theme, "THEME PERFORMANCE"),
            form::options(
                theme,
                choices,
                Some(self.cx.layout.theme_performance.index() as usize),
                true,
                |index| theme_message(Event::Performance(index as i32)),
            ),
            switch(
                "Reduce motion",
                self.cx.layout.reduce_motion,
                Some(theme_message(Event::ReduceMotion(
                    !self.cx.layout.reduce_motion,
                ))),
            )
            .description("Keep installed-theme animation and motion still.")
            .view(self.cx.look),
            switch(
                "Transitions between channels",
                self.cx.layout.theme_transitions,
                Some(theme_message(Event::ThemeTransitions(
                    !self.cx.layout.theme_transitions,
                ))),
            )
            .description("Allow an installed theme's transition when the open channel changes.")
            .view(self.cx.look),
        ])
    }

    /// The themes as tiles, as many 150 px or wider across as fit.
    fn theme_tiles(self, include_built_in: bool) -> Element<'a, Message> {
        iced::widget::responsive(move |size| {
            let columns = ((size.width + TILE_GAP) / (TILE + TILE_GAP))
                .floor()
                .max(1.0) as usize;
            let width = (size.width - (columns - 1) as f32 * TILE_GAP) / columns as f32;
            let built_in = if include_built_in {
                palette::BUILT_IN.as_slice()
            } else {
                &[]
            };
            let cards: Vec<(String, String, bool, palette::Theme)> =
                built_in
                    .iter()
                    .map(|built_in| {
                        (
                            built_in.id.to_owned(),
                            built_in.name.to_owned(),
                            false,
                            built_in.theme,
                        )
                    })
                    .chain(
                        self.cx.themes.custom.iter().map(|custom| {
                            (custom.id.clone(), custom.name.clone(), true, custom.theme)
                        }),
                    )
                    .collect();
            let mut rows = column![].spacing(TILE_GAP);
            for chunk in cards.chunks(columns) {
                let mut line = row![].spacing(TILE_GAP);
                for (id, name, custom, palette) in chunk {
                    line = line.push(self.theme_tile(id, name, *custom, palette, width));
                }
                rows = rows.push(line);
            }
            rows.into()
        })
        .height(Length::Shrink)
        .into()
    }

    /// A theme to choose: a small window in its colours, its name under it,
    /// an accent ring and a tick on the one shown, and a pencil on the
    /// person's own.
    fn theme_tile(
        self,
        id: &str,
        name: &str,
        custom: bool,
        palette: &palette::Theme,
        width: f32,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let current = self.cx.themes.chosen_id() == id;
        let preview = preview(palette, width - 16.0);
        let mut label = row![
            text(name.to_owned())
                .size(14)
                .font(if current { BOLD } else { MEDIUM })
                .color(if current { theme.text } else { theme.secondary })
                .wrapping(text::Wrapping::None)
                .width(Length::Fill)
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .height(24);
        if current {
            label = label.push(self.cx.look.glyph(Icon::Check, 16, theme.accent));
        }
        let (accent, faint, line, hover) = (theme.accent, theme.faint, theme.line, theme.hover);
        let face = button(
            column![preview, container(label).padding([0, 2])]
                .spacing(6)
                .padding(8),
        )
        .padding(0)
        .width(width)
        .height(TILE_HEIGHT)
        .on_press(theme_message(Event::Choose(id.to_owned())))
        .style(move |_, status| {
            let lit = lights(status);
            button::Style {
                background: lit.then(|| hover.into()),
                border: Border {
                    color: if current {
                        accent
                    } else if lit {
                        faint
                    } else {
                        line
                    },
                    width: if current { 2.0 } else { 1.0 },
                    radius: 10.0.into(),
                },
                ..Default::default()
            }
        });
        if !custom {
            return face.into();
        }
        let pencil = self
            .cx
            .look
            .icon_button(
                Icon::Pencil,
                "Edit",
                theme_message(Event::Edit(id.to_owned())),
            )
            .size(24.0, 14);
        stack![face, placed(pencil, width - 8.0 - 24.0, 8.0 + 78.0 + 6.0)].into()
    }

    /// The editor: the name with Save and Cancel, what a new theme starts
    /// from, Dark, each colour's field, and Delete for a saved one.
    fn editor_view(self, editor: &'a Editor, starts_open: bool) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let mut parts = vec![
            form::title(theme, "Themes"),
            form::group([
                form::heading(
                    theme,
                    if editor.editing.is_some() {
                        "EDIT THEME"
                    } else {
                        "NEW THEME"
                    },
                ),
                row![
                    form::field(theme, NAME, "Name", &editor.name)
                        .on_input(|name| theme_message(Event::Name(name)))
                        .on_submit(theme_message(Event::Save)),
                    action(
                        theme,
                        "Save",
                        Kind::Primary,
                        editor.valid().then(|| theme_message(Event::Save))
                    ),
                    action(
                        theme,
                        "Cancel",
                        Kind::Plain,
                        Some(theme_message(Event::Cancel))
                    )
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
            ]),
        ];
        if editor.editing.is_none() {
            let names: Vec<String> = palette::starts(&self.cx.themes.custom)
                .map(|start| start.name.to_owned())
                .collect();
            parts.push(form::group([
                form::heading(theme, "START FROM"),
                form::dropdown(
                    names,
                    Some(editor.start),
                    starts_open,
                    crate::app::settings(SettingsEvent::Drop(Drop::Start)),
                    |index| theme_message(Event::Start(index)),
                )
                .view(self.cx.look),
            ]));
        }
        parts.push(
            switch(
                "Dark",
                editor.theme.dark,
                Some(theme_message(Event::Dark(!editor.theme.dark))),
            )
            .description("Drop-down lists and scroll bars draw dark to match.")
            .view(self.cx.look),
        );
        let colours = iced::widget::responsive(move |size| {
            let columns = if size.width >= TWO_COLUMNS { 2 } else { 1 };
            let per = editor.fields.len().div_ceil(columns);
            let mut line = row![].spacing(COLOUR_GAP);
            for column_index in 0..columns {
                let mut cells = column![];
                for index in column_index * per..((column_index + 1) * per).min(editor.fields.len())
                {
                    let (name, hex, wrong) = &editor.fields[index];
                    let shown = editor.theme.colour(index).map_or(Color::TRANSPARENT, color);
                    cells = cells.push(form::colour_row(
                        theme,
                        name,
                        shown,
                        hex,
                        *wrong,
                        move |text| theme_message(Event::Colour(index, text)),
                    ));
                }
                line = line.push(container(cells).width(Length::FillPortion(1)));
            }
            line.into()
        })
        .height(Length::Shrink);
        parts.push(form::group([
            form::heading(theme, "COLOURS"),
            form::note(
                theme,
                "Each as #rgb, #rrggbb or #rrggbbaa. The window shows a change as you type it.",
            ),
            colours.into(),
        ]));
        if editor.editing.is_some() {
            parts.push(form::buttons([action(
                theme,
                if editor.confirming {
                    "Delete it for good"
                } else {
                    "Delete theme"
                },
                Kind::Danger,
                Some(theme_message(Event::Delete)),
            )
            .small()
            .icon(
                self.cx
                    .look
                    .icon(Icon::Trash, 16, crate::theme::rgb(theme.on_danger)),
            )
            .into()]));
        }
        form::page(parts)
    }
}

/// What a theme copied from another is called until it's named.
fn copy_name(name: &str) -> String {
    format!("{name} copy")
}

/// A theme's small window, 78 px tall: the rail with three servers, the
/// sidebar's channels, two messages and the composer, in its colours.
fn preview<'a>(palette: &palette::Theme, width: f32) -> Element<'a, Message> {
    let c = |colour| color(colour);
    let (rail, sidebar, chat, raised, accent, ink, muted) = (
        c(palette.rail),
        c(palette.sidebar),
        c(palette.chat),
        c(palette.raised),
        c(palette.accent),
        c(palette.text),
        c(palette.muted),
    );
    let blob = |w: f32, h: f32, colour: Color, radius: f32| -> Element<'a, Message> {
        container(Space::new())
            .width(w)
            .height(h)
            .style(move |_| crate::style::rounded(colour, radius))
            .into()
    };
    let mut layers = stack![blob(width, 78.0, chat, 6.0)];
    layers = layers.push(blob(16.0, 78.0, rail, 0.0));
    for index in 0..3 {
        let (colour, radius) = if index == 0 {
            (accent, 3.0)
        } else {
            (raised, 5.0)
        };
        layers = layers.push(placed(
            blob(10.0, 10.0, colour, radius),
            3.0,
            6.0 + index as f32 * 13.0,
        ));
    }
    layers = layers.push(placed(blob(38.0, 78.0, sidebar, 0.0), 16.0, 0.0));
    for index in 0..4 {
        let w = if index == 1 {
            24.0
        } else {
            18.0 + (index % 2) as f32 * 4.0
        };
        let colour = if index == 1 { ink } else { muted };
        layers = layers.push(placed(
            blob(w, 4.0, colour, 2.0),
            22.0,
            8.0 + index as f32 * 11.0,
        ));
    }
    let room = width - 66.0;
    for index in 0..2 {
        let y = 8.0 + index as f32 * 22.0;
        let first = index == 0;
        layers = layers
            .push(placed(
                blob(10.0, 10.0, if first { accent } else { muted }, 5.0),
                60.0,
                y,
            ))
            .push(placed(
                blob(if first { 22.0 } else { 16.0 }, 4.0, ink, 2.0),
                74.0,
                y + 1.0,
            ))
            .push(placed(
                blob(
                    (room - 14.0).min(if first { 52.0 } else { 40.0 }),
                    3.0,
                    muted,
                    1.5,
                ),
                74.0,
                y + 8.0,
            ));
    }
    layers = layers
        .push(placed(blob(room, 12.0, raised, 4.0), 60.0, 78.0 - 18.0))
        .push(placed(
            blob(6.0, 6.0, accent, 3.0),
            60.0 + room - 10.0,
            78.0 - 18.0,
        ));
    container(layers.width(width).height(78))
        .clip(true)
        .style(|_| iced::widget::container::Style {
            border: crate::style::round(6),
            ..Default::default()
        })
        .into()
}

impl<'a> Page<'a> {
    /// Appearance's "Use the system title bar", where Yapfast can draw its
    /// own: the desktop's way until it's chosen.
    pub(super) fn title_bar_switch(self) -> Element<'a, Message> {
        if !crate::window_bar::offered() {
            return Space::new().into();
        }
        let system = !self.cx.own_title_bar;
        switch(
            "Use the system title bar",
            system,
            Some(crate::app::settings(SettingsEvent::Pref(
                super::pages::Pref::SystemTitleBar(!system),
            ))),
        )
        .description("Your desktop draws the window's title bar and buttons, as on its other windows. Off, Yapfast draws its own.")
        .view(self.cx.look)
    }
}
