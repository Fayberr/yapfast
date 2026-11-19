//! Settings → Themes' marketplace, GitHub URL and install-review flow.
//!
//! This is deliberately a self-contained controller for the Settings host
//! to map into its own message. Network work stays in iced [`Task`]s on the
//! core background runtime; checked bytes stay in [`Review`] until Install.
//! Reviewed bytes remain protected until Install or the explicit Try flow.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use iced::{
    Alignment, Element, Length, Task,
    widget::{Space, button, column, container, image, row, text},
};
use yapfast_core::{
    runtime::BackgroundRuntime,
    themes::{
        check::plain::{self, Weight},
        install::Fetched,
        remote::{Catalog, Listing, Repository},
    },
};

use crate::{
    controls::{self, Kind},
    form,
    layout::{self, Layout},
    look::Look,
    theme_remote::{self, Preview, Previews},
    themes::{SoundPrefs, Themes},
};

const SEARCH: &str = "theme-marketplace-search";
const URL: &str = "theme-repository-url";
const MOST_NOTES: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Installed,
    Marketplace,
    AddFromUrl,
}

/// A checked theme waiting at the protected install review. Nothing in the
/// fetched package is persisted until [`Event::Install`].
pub struct Review {
    pub fetched: Fetched,
    pub previews: Previews,
    pub notification_sounds: bool,
    pub interface_sounds: bool,
    pub content: Option<String>,
    pub notify_on: bool,
    pub interface_on: bool,
    pub content_on: bool,
    stock: crate::theme::Theme,
}

impl Review {
    fn new(fetched: Fetched, previews: Previews) -> Self {
        let package = fetched.package();
        let mut notification_sounds = false;
        let mut interface_sounds = false;
        let mut content = None;
        for resolved in package
            .modes()
            .into_iter()
            .filter_map(|mode| package.resolve(mode).ok())
        {
            notification_sounds |= resolved.sounds.keys().any(|slot| !slot.interface());
            interface_sounds |= resolved.sounds.keys().any(|slot| slot.interface());
            if content.is_none() {
                content = resolved.content.phrase();
            }
        }
        Self {
            fetched,
            previews,
            notification_sounds,
            interface_sounds,
            content,
            // The owner's safe review defaults: notification replacements
            // on, interface cues and people's content off.
            notify_on: true,
            interface_on: false,
            content_on: false,
            stock: crate::themes::theme(
                "theme-review",
                &yapfast_core::theme::find("bonfire")
                    .map_or(yapfast_core::theme::YAP, |built_in| built_in.theme),
            ),
        }
    }

    #[cfg(test)]
    pub fn notes(&self) -> String {
        format_notes(&self.fetched.package().notes)
    }

    pub fn modes(&self) -> &'static str {
        match self.fetched.package().modes().as_slice() {
            [
                yapfast_core::themes::Mode::Dark,
                yapfast_core::themes::Mode::Light,
            ] => "Dark and light",
            [yapfast_core::themes::Mode::Light] => "Light",
            _ => "Dark",
        }
    }
}

/// The two remote tabs' state. `catalog == None` is either not opened yet
/// or loading; `catalog_loading` distinguishes them.
#[derive(Default)]
pub struct Marketplace {
    pub tab: Tab,
    pub query: String,
    pub url: String,
    pub catalog: Option<Catalog>,
    pub catalog_loading: bool,
    pub catalog_error: Option<String>,
    pub fetching: Option<String>,
    pub error: Option<String>,
    pub review: Option<Review>,
    pub previews: HashMap<String, Preview>,
    /// Set after a successful install for the Settings host to apply/redraw.
    pub installed: Option<String>,
    pub theme_changed: bool,
    pub theme_applied: bool,
    pub gallery: bool,
    pub cost_details: HashSet<String>,
    trial_permissions: Option<(SoundPrefs, bool)>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Tab(Tab),
    Search(String),
    ToggleGallery(bool),
    ToggleCostDetails(String),
    RetryCatalog,
    CatalogLoaded(Result<Catalog, String>),
    PreviewLoaded(String, Option<Preview>),
    Url(String),
    FindUrl,
    FetchListed(String),
    Fetched(Box<Result<(Fetched, Previews), String>>),
    ReviewNotify(bool),
    ReviewInterface(bool),
    ReviewContent(bool),
    CancelReview,
    TryReview,
    TryInstalled(String),
    KeepTrial,
    RevertTrial,
    RollBack(String),
    Uninstall(String),
    Install,
    ClearInstalled,
}

impl Marketplace {
    /// The stock Bonfire look while the protected install review is open.
    /// Settings exposes this as its preview so the whole window, not merely
    /// the review card, gives up the untrusted theme's styling.
    pub fn protected_theme(&self) -> Option<&crate::theme::Theme> {
        self.review.as_ref().map(|review| &review.stock)
    }

    /// Handles one marketplace action. The caller maps this task's [`Event`]
    /// back into its Settings message.
    pub fn update(
        &mut self,
        event: Event,
        runtime: Arc<BackgroundRuntime>,
        root: &Path,
        layout: &mut Layout,
        themes: &mut Themes,
    ) -> Task<Event> {
        match event {
            Event::Open => {
                if self.tab == Tab::Marketplace && self.catalog.is_none() && !self.catalog_loading {
                    return self.load_catalog(runtime);
                }
            }
            Event::Tab(tab) => {
                self.tab = tab;
                self.error = None;
                if tab == Tab::Marketplace && self.catalog.is_none() && !self.catalog_loading {
                    return self.load_catalog(runtime);
                }
            }
            Event::Search(query) => self.query = query,
            Event::ToggleGallery(gallery) => self.gallery = gallery,
            Event::ToggleCostDetails(key) => {
                if !self.cost_details.remove(&key) {
                    self.cost_details.insert(key);
                }
            }
            Event::RetryCatalog => return self.load_catalog(runtime),
            Event::CatalogLoaded(result) => {
                self.catalog_loading = false;
                match result {
                    Ok(catalog) => {
                        self.catalog_error = None;
                        let urls = catalog
                            .themes
                            .iter()
                            .filter(|listing| {
                                !listing.preview.is_empty()
                                    && !self.previews.contains_key(&listing.preview)
                                    && !themes.installed.iter().any(|installed| {
                                        installed.id
                                            == format!("{}{}", crate::themes::INSTALLED, listing.id)
                                    })
                            })
                            .map(|listing| listing.preview.clone())
                            .collect::<Vec<_>>();
                        self.catalog = Some(catalog);
                        return Task::batch(urls.into_iter().map(|url| {
                            let runtime = runtime.clone();
                            Task::perform(
                                async move {
                                    let preview = theme_remote::preview(runtime, url.clone()).await;
                                    (url, preview)
                                },
                                |(url, preview)| Event::PreviewLoaded(url, preview),
                            )
                        }));
                    }
                    Err(error) => {
                        self.catalog = None;
                        self.catalog_error = Some(error);
                    }
                }
            }
            Event::PreviewLoaded(url, preview) => {
                if let Some(preview) = preview {
                    self.previews.insert(url, preview);
                }
            }
            Event::Url(url) => {
                self.url = url;
                self.error = None;
            }
            Event::FindUrl => {
                if self.fetching.is_some() {
                    return Task::none();
                }
                let repository = match Repository::parse(&self.url) {
                    Ok(repository) => repository,
                    Err(error) => {
                        self.error = Some(error);
                        return Task::none();
                    }
                };
                self.fetching = Some(self.url.clone());
                self.error = None;
                return Task::perform(theme_remote::fetch(runtime, repository, None), |result| {
                    Event::Fetched(Box::new(result))
                });
            }
            Event::FetchListed(id) => {
                if self.fetching.is_some() {
                    return Task::none();
                }
                let Some(listing) = self
                    .catalog
                    .as_ref()
                    .and_then(|catalog| catalog.themes.iter().find(|listing| listing.id == id))
                    .cloned()
                else {
                    return Task::none();
                };
                self.fetching = Some(id);
                self.error = None;
                return Task::perform(
                    theme_remote::fetch(runtime, listing.repository(), Some(listing)),
                    |result| Event::Fetched(Box::new(result)),
                );
            }
            Event::Fetched(result) => {
                self.fetching = None;
                match *result {
                    Ok((fetched, previews)) => {
                        self.error = None;
                        self.review = Some(Review::new(fetched, previews));
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Event::ReviewNotify(on) => {
                if let Some(review) = &mut self.review {
                    review.notify_on = on;
                }
            }
            Event::ReviewInterface(on) => {
                if let Some(review) = &mut self.review {
                    review.interface_on = on;
                }
            }
            Event::ReviewContent(on) => {
                if let Some(review) = &mut self.review {
                    review.content_on = on;
                }
            }
            Event::CancelReview => self.review = None,
            Event::TryReview => {
                let Some(review) = self.review.take() else {
                    return Task::none();
                };
                let prefs = SoundPrefs {
                    on: review.notify_on,
                    interface: review.interface_on,
                    ..SoundPrefs::default()
                };
                if review.content_on {
                    layout.theme_content.insert("trial".into());
                } else {
                    layout.theme_content.remove("trial");
                }
                layout.theme_sounds.insert("trial".into(), prefs);
                match themes.try_reviewed(&review.fetched, layout) {
                    Ok(()) => {
                        themes.set_sound_prefs("trial", prefs);
                        self.trial_permissions = Some((prefs, review.content_on));
                        self.theme_changed = true;
                    }
                    Err(error) => {
                        layout.theme_sounds.remove("trial");
                        layout.theme_content.remove("trial");
                        self.error = Some(error);
                        self.review = Some(review);
                    }
                }
            }
            Event::TryInstalled(id) => match themes.try_installed(&id) {
                Ok(()) => {
                    self.trial_permissions = None;
                    self.theme_changed = true;
                }
                Err(error) => self.error = Some(error),
            },
            Event::KeepTrial => match themes.keep_trial(root, layout) {
                Ok(Some(id)) => {
                    if let Some((prefs, content)) = self.trial_permissions.take() {
                        if prefs == SoundPrefs::default() {
                            layout.theme_sounds.remove(&id);
                        } else {
                            layout.theme_sounds.insert(id.clone(), prefs);
                        }
                        themes.set_sound_prefs(&id, prefs);
                        if content {
                            layout.theme_content.insert(id.clone());
                        } else {
                            layout.theme_content.remove(&id);
                        }
                        layout::store(root, "theme_sounds", &layout.theme_sounds);
                        layout::store(root, "theme_content", &layout.theme_content);
                    }
                    layout.theme_sounds.remove("trial");
                    layout.theme_content.remove("trial");
                    let before = themes.chosen.clone();
                    themes.choose(root, &id);
                    layout.previous_theme = before;
                    layout.theme = Some(id.clone());
                    self.installed = Some(id);
                    self.tab = Tab::Installed;
                    self.theme_applied = true;
                }
                Ok(None) => {}
                Err(error) => self.error = Some(error),
            },
            Event::RevertTrial => {
                if themes.revert_trial().is_some() {
                    layout.theme_sounds.remove("trial");
                    layout.theme_content.remove("trial");
                    self.trial_permissions = None;
                    self.theme_changed = true;
                }
            }
            Event::RollBack(id) => match themes.roll_back(root, layout, &id) {
                Ok(()) => self.theme_changed = true,
                Err(error) => self.error = Some(error),
            },
            Event::Uninstall(id) => match themes.uninstall(root, layout, &id) {
                Ok(changed) => self.theme_changed |= changed,
                Err(error) => self.error = Some(error),
            },
            Event::Install => {
                let Some(review) = self.review.take() else {
                    return Task::none();
                };
                let id = match themes.install_fetched(root, layout, &review.fetched) {
                    Ok(id) => id,
                    Err(error) => {
                        self.error = Some(error);
                        self.review = Some(review);
                        return Task::none();
                    }
                };

                let prefs = SoundPrefs {
                    on: review.notify_on,
                    interface: review.interface_on,
                    ..SoundPrefs::default()
                };
                if prefs == SoundPrefs::default() {
                    layout.theme_sounds.remove(&id);
                } else {
                    layout.theme_sounds.insert(id.clone(), prefs);
                }
                themes.set_sound_prefs(&id, prefs);
                if review.content_on {
                    layout.theme_content.insert(id.clone());
                } else {
                    layout.theme_content.remove(&id);
                }
                layout::store(root, "theme_sounds", &layout.theme_sounds);
                layout::store(root, "theme_content", &layout.theme_content);

                let before = themes.chosen.clone();
                themes.choose(root, &id);
                if before.as_deref() != Some(id.as_str()) {
                    layout.previous_theme = before;
                }
                layout.theme = Some(id.clone());
                self.installed = Some(id);
                self.tab = Tab::Installed;
                self.theme_applied = true;
            }
            Event::ClearInstalled => self.installed = None,
        }
        Task::none()
    }

    fn load_catalog(&mut self, runtime: Arc<BackgroundRuntime>) -> Task<Event> {
        self.catalog_loading = true;
        self.catalog_error = None;
        Task::perform(theme_remote::catalog(runtime), Event::CatalogLoaded)
    }

    /// Marketplace entries that are not installed and match the current
    /// search by name, author or description.
    pub fn listed<'a>(&'a self, themes: &'a Themes) -> impl Iterator<Item = &'a Listing> + 'a {
        let query = self.query.trim().to_lowercase();
        self.catalog
            .iter()
            .flat_map(|catalog| &catalog.themes)
            .filter(move |listing| {
                !themes.installed.iter().any(|installed| {
                    installed.id == format!("{}{}", crate::themes::INSTALLED, listing.id)
                }) && (query.is_empty()
                    || listing.name.to_lowercase().contains(&query)
                    || listing.author.to_lowercase().contains(&query)
                    || listing.description.to_lowercase().contains(&query))
            })
    }

    /// Draws the selected remote tab or its review. The Settings host owns
    /// the full Installed / Marketplace / Add from URL / Create tab row.
    /// While [`Self::review`] is present, it passes the stock look so the
    /// protected review cannot disguise its permissions or Install action.
    pub fn view<'a>(&'a self, look: Look<'a>, themes: &'a Themes) -> Element<'a, Event> {
        if let Some(review) = &self.review {
            return self.review_view(
                Look {
                    theme: &review.stock,
                    ..look
                },
                review,
            );
        }
        let theme = look.theme;
        let error: Element<'a, Event> = self.error.as_ref().map_or_else(
            || Space::new().height(0).into(),
            |error| form::note_in(error.clone(), theme.danger),
        );
        let body = match self.tab {
            Tab::Installed => Space::new().height(0).into(),
            Tab::Marketplace => self.marketplace_view(look, themes),
            Tab::AddFromUrl => self.url_view(look),
        };
        form::page([error, body])
    }

    fn marketplace_view<'a>(&'a self, look: Look<'a>, themes: &'a Themes) -> Element<'a, Event> {
        let theme = look.theme;
        let search = form::field(theme, SEARCH, "Search the marketplace", &self.query)
            .on_input(Event::Search);
        if self.catalog_loading {
            return form::group([search.into(), form::note(theme, "Loading the marketplace…")]);
        }
        if let Some(error) = &self.catalog_error {
            return form::group([
                search.into(),
                form::heading(theme, "THE MARKETPLACE CAN'T BE SHOWN RIGHT NOW"),
                form::note_in(error.clone(), theme.danger),
                controls::action(theme, "Try again", Kind::Primary, Some(Event::RetryCatalog))
                    .into(),
            ]);
        }
        let listings: Vec<_> = self.listed(themes).collect();
        if listings.is_empty() {
            let empty_query = self.query.trim().is_empty();
            let words = if empty_query {
                "Every theme the marketplace lists is installed.".to_owned()
            } else {
                format!(
                    "No themes match \u{201c}{}\u{201d}. Try another word or author.",
                    self.query
                )
            };
            let mut buttons = row![].spacing(8);
            if !empty_query {
                buttons = buttons.push(
                    controls::action(
                        theme,
                        "Clear search",
                        Kind::Primary,
                        Some(Event::Search(String::new())),
                    )
                    .small(),
                );
            }
            buttons = buttons.push(
                controls::action(
                    theme,
                    "Add from URL",
                    Kind::Plain,
                    Some(Event::Tab(Tab::AddFromUrl)),
                )
                .small(),
            );
            let notice = container(column![form::note(theme, words), buttons].spacing(10))
                .padding(14)
                .width(Length::Fill)
                .style(move |_| crate::style::rounded(theme.raised, 8));
            return form::group([search.into(), notice.into()]);
        }
        let mut grid = column![].spacing(12);
        for chunk in listings.chunks(2) {
            let mut line = row![].spacing(12);
            for listing in chunk {
                line = line.push(
                    container(self.listing_view(theme, listing)).width(Length::FillPortion(1)),
                );
            }
            if chunk.len() == 1 {
                line = line.push(Space::new().width(Length::FillPortion(1)));
            }
            grid = grid.push(line);
        }
        form::group([search.into(), grid.into()])
    }

    fn listing_view<'a>(
        &'a self,
        theme: &'a crate::theme::Theme,
        listing: &'a Listing,
    ) -> Element<'a, Event> {
        let preview: Element<'a, Event> = self.previews.get(&listing.preview).map_or_else(
            || Space::new().height(0).into(),
            |preview| {
                image(preview.handle.clone())
                    .height(160)
                    .width(Length::Fill)
                    .content_fit(iced::ContentFit::Cover)
                    .into()
            },
        );
        let finding = self.fetching.as_deref() == Some(listing.id.as_str());
        let action = controls::action(
            theme,
            if finding { "Fetching…" } else { "Install" },
            Kind::Primary,
            (!finding && self.fetching.is_none()).then(|| Event::FetchListed(listing.id.clone())),
        );
        container(
            column![
                preview,
                text(listing.name.clone()).size(17).color(theme.text),
                text(format!("by {} · {}", listing.author, listing.modes()))
                    .size(13)
                    .color(theme.muted),
                if listing.description.is_empty() {
                    Space::new().height(0).into()
                } else {
                    form::note(theme, listing.description.clone())
                },
                action,
            ]
            .spacing(8),
        )
        .padding(12)
        .width(Length::Fill)
        .style(move |_| crate::style::rounded(theme.raised, 8))
        .into()
    }

    fn url_view<'a>(&'a self, look: Look<'a>) -> Element<'a, Event> {
        let theme = look.theme;
        let finding = self.fetching.is_some();
        let field = form::field(theme, URL, "https://github.com/ada/vaporwave", &self.url)
            .on_input(Event::Url)
            .on_submit(Event::FindUrl);
        form::page([
            form::group([
                form::heading(theme, "GITHUB REPOSITORY"),
                field.into(),
                controls::action(
                    theme,
                    if finding { "Finding…" } else { "Find theme" },
                    Kind::Primary,
                    (!finding && !self.url.trim().is_empty()).then_some(Event::FindUrl),
                )
                .into(),
                form::note(
                    theme,
                    "The address of a theme's public GitHub repository. A branch, tag or folder in the address works too.",
                ),
            ]),
            form::rule(theme),
            form::group([
                form::heading(theme, "WHAT HAPPENS NEXT"),
                form::note(
                    theme,
                    "1. Yapfast fetches it straight from the repository at its current commit.",
                ),
                form::note(
                    theme,
                    "2. Every file is checked. A theme is data, never code.",
                ),
                form::note(
                    theme,
                    "3. You review its changes and running cost before anything is installed.",
                ),
            ]),
        ])
    }

    fn review_view<'a>(&'a self, look: Look<'a>, review: &'a Review) -> Element<'a, Event> {
        let theme = look.theme;
        let package = review.fetched.package();
        let mut previews = row![].spacing(8);
        for preview in &review.previews {
            previews = previews.push(
                image(preview.handle.clone())
                    .width(240)
                    .height(150)
                    .content_fit(iced::ContentFit::Cover),
            );
        }
        let source = format!(
            "{} at {}",
            review.fetched.source,
            review.fetched.rev.chars().take(7).collect::<String>()
        );
        let trust = if review.fetched.listed {
            "In the marketplace · No code · no permissions"
        } else {
            "Unreviewed source · No code · no permissions"
        };
        let mut parts: Vec<Element<'a, Event>> = vec![
            text(format!("{} · v{}", package.name, package.version))
                .size(20)
                .color(theme.text)
                .into(),
            form::note(theme, format!("by {}", package.authors.join(", "))),
        ];
        if !package.description.is_empty() {
            parts.push(form::note(theme, package.description.clone()));
        }
        if !review.previews.is_empty() {
            parts.push(previews.into());
        }
        parts.extend([
            form::note_in(
                trust,
                if review.fetched.listed {
                    theme.live
                } else {
                    theme.loud
                },
            ),
            form::group([
                form::heading(theme, "FROM"),
                form::note(theme, source),
                form::heading(theme, "WORKS IN"),
                form::note(theme, review.modes()),
            ]),
        ]);
        if let Some(based_on) = &package.based_on {
            parts.push(form::group([
                form::heading(theme, "REMIXES"),
                form::note(theme, based_on.clone()),
            ]));
        }
        parts.push(form::group([
            form::heading(theme, "TO RUN"),
            cost_line(
                theme,
                &review.fetched.report.cost.text(),
                &package.notes,
                self.cost_details.contains("review"),
                Event::ToggleCostDetails("review".into()),
            ),
        ]));
        if review.notification_sounds {
            parts.push(
                form::switch(
                    "Notification sounds",
                    review.notify_on,
                    Some(Event::ReviewNotify(!review.notify_on)),
                )
                .description(
                    "Its sounds replace Yapfast's for messages, mentions, calls and the rest.",
                )
                .view(look),
            );
        }
        if review.interface_sounds {
            parts.push(
                form::switch(
                    "Interface sounds — clicks, hovers, menus, send",
                    review.interface_on,
                    Some(Event::ReviewInterface(!review.interface_on)),
                )
                .description("Only while Yapfast is in front. You can change this later in Themes.")
                .view(look),
            );
        }
        if let Some(content) = &review.content {
            parts.push(
                form::switch(
                    "Let the theme style people's profiles and media",
                    review.content_on,
                    Some(Event::ReviewContent(!review.content_on)),
                )
                .description(format!(
                    "It can draw {content}. Off, everyone's own looks show."
                ))
                .view(look),
            );
        }
        parts.push(
            row![
                controls::action(theme, "Cancel", Kind::Ghost, Some(Event::CancelReview)),
                Space::new().width(Length::Fill),
                controls::action(
                    theme,
                    "Try for 30 seconds",
                    Kind::Plain,
                    Some(Event::TryReview)
                ),
                controls::action(theme, "Install", Kind::Primary, Some(Event::Install)),
            ]
            .spacing(8)
            .into(),
        );
        form::page(parts)
    }
}

/// A theme's weight badge, plain-language summary and expandable cost breakdown
/// + package notes, shared across Installed and Review screens.
pub(super) fn cost_line<'a, M: Clone + 'a>(
    theme: &'a crate::theme::Theme,
    cost: &str,
    notes: &[String],
    expanded: bool,
    on_toggle: M,
) -> Element<'a, M> {
    let parsed = plain::plain(cost);
    if parsed.is_none() && notes.is_empty() {
        return Space::new().height(0).into();
    }
    let (dot_colour, summary, rows) = match &parsed {
        Some(p) => (
            match p.weight {
                Weight::Light => theme.live,
                Weight::Moderate => theme.loud,
                Weight::Heavy => theme.danger,
            },
            p.summary.clone(),
            p.rows.as_slice(),
        ),
        None => (theme.muted, "Package notes".to_owned(), [].as_slice()),
    };
    let dot = container(Space::new())
        .width(7)
        .height(7)
        .style(move |_| crate::style::rounded(dot_colour, 3.5));
    let mut header = row![
        dot,
        text(summary)
            .size(theme.type_size("small", 12.0))
            .font(theme.font("small", crate::fonts::MEDIUM))
            .color(theme.secondary)
            .width(Length::Fill),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if !rows.is_empty() || !notes.is_empty() {
        header = header.push(
            button(
                text(if expanded { "Less" } else { "Details" })
                    .size(theme.type_size("small", 12.0))
                    .font(theme.font("small", crate::fonts::MEDIUM)),
            )
            .padding([2, 6])
            .on_press(on_toggle)
            .style(controls::ink_style(theme.accent_text(), theme.text)),
        );
    }
    if !expanded {
        return header.into();
    }
    let mut detail = column![header].spacing(4);
    for (label, value) in rows {
        detail = detail.push(
            row![
                text(format!("{label}:"))
                    .size(theme.type_size("small", 12.0))
                    .font(theme.font("small", crate::fonts::MEDIUM))
                    .color(theme.muted),
                text(value.clone())
                    .size(theme.type_size("small", 12.0))
                    .font(theme.font("small", crate::fonts::REGULAR))
                    .color(theme.secondary),
            ]
            .spacing(6),
        );
    }
    if !notes.is_empty() {
        let shown = format_notes(notes);
        detail = detail.push(
            text(format!("Left out by this Yapfast:\n{shown}"))
                .size(theme.type_size("small", 12.0))
                .font(theme.font("small", crate::fonts::REGULAR))
                .color(theme.muted),
        );
    }
    detail.into()
}

fn format_notes(notes: &[String]) -> String {
    let mut shown = notes
        .iter()
        .take(MOST_NOTES)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    if notes.len() > MOST_NOTES {
        shown.push_str(&format!("\nand {} more", notes.len() - MOST_NOTES));
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use yapfast_core::themes::check::Files;

    fn fetched() -> Fetched {
        let mut fetched = Fetched::new(
            "https://github.com/ada/plum".into(),
            String::new(),
            "main".into(),
            "0123456789abcdef0123456789abcdef01234567".into(),
            Files {
                manifest: r##"{
                    "theme_api": 1,
                    "id": "com.example.plum",
                    "name": "Plum",
                    "version": "1.0.0",
                    "variants": { "dark": { "base": "yap", "colours": { "chat": "#2a1a3a" } } }
                }"##
                .into(),
                ..Files::default()
            },
        )
        .unwrap();
        fetched.report.package.as_mut().unwrap().notes =
            (1..=10).map(|number| number.to_string()).collect();
        fetched
    }

    #[test]
    fn review_uses_owner_defaults_and_bounds_notes() {
        let review = Review::new(fetched(), Vec::new());
        assert!(review.notify_on);
        assert!(!review.interface_on);
        assert!(!review.content_on);
        assert_eq!(review.modes(), "Dark");
        assert_eq!(review.notes(), "1\n2\n3\n4\n5\n6\n7\n8\nand 2 more");
    }

    #[test]
    fn marketplace_searches_names_authors_and_descriptions() {
        let root = tempfile::tempdir().unwrap();
        let themes = Themes::read(root.path(), &Layout::default());
        let listing = Listing {
            id: "com.example.plum".into(),
            name: "Plum".into(),
            author: "Ada".into(),
            description: "A quiet purple theme".into(),
            ..Listing::default()
        };
        let mut market = Marketplace {
            catalog: Some(Catalog {
                themes: vec![listing],
                skipped: Vec::new(),
            }),
            ..Marketplace::default()
        };
        for query in ["plum", "ADA", "purple"] {
            market.query = query.into();
            assert_eq!(market.listed(&themes).count(), 1);
        }
        market.query = "orange".into();
        assert_eq!(market.listed(&themes).count(), 0);
    }
}
