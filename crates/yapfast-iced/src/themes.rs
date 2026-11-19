//! The themes the window shows: one of the core's colour themes, or an
//! installed theme package resolved over one of them, made the kit's
//! [`Theme`]. See docs/themes.md.
//!
//! The theme is the one `layout.json` names: a built-in id or one of the
//! person's own (`custom_themes`), Bonfire when it names none, Yap for an
//! id Yapfast doesn't know. The window-control command `theme ID` chooses
//! another and saves it, as Settings → Appearance and Themes do; their iced
//! views share `settings/appearance.rs`.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use iced::Color;
use yapfast_core::themes::package::art::{self, ArtSurface, Source};
use yapfast_core::{
    ring,
    sounds::{self, Sound, ThemedSound},
    theme::{self as palette, Colour, Custom},
    themes::{
        Mode, Performance, Resolved, check, check::Strictness, chrome, content, install,
        layout as theme_layout, media, options, package,
    },
};

use crate::{
    layout::{self, Layout},
    theme::Theme,
};

/// The theme shown when `layout.json` names none.
const DEFAULT: &str = "bonfire";
/// Installed package ids are kept apart from built-in and custom ids.
pub const INSTALLED: &str = "installed:";

/// An installed package as Settings → Themes and Appearance need it.
pub struct Installed {
    /// Its id as `layout.json` keeps it, including [`INSTALLED`].
    pub id: String,
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    pub description: String,
    pub modes: Vec<Mode>,
    pub notes: Vec<String>,
    pub options: Vec<options::ThemeOption>,
    pub choices: options::Choices,
    /// The revision retained by core for Roll back, when there is one.
    pub previous: Option<String>,
    /// What this theme remixes, if named in `theme.json`.
    pub based_on: Option<String>,
    /// Core's cost summary line for Settings → Themes.
    pub cost: String,
    /// Decoded thumbnail of `package.preview[0]`, if present and readable.
    pub preview: Option<iced::advanced::image::Handle>,
    /// Resolved palette for miniature colour previews when no preview image exists.
    pub palette_preview: palette::Theme,
    folder: PathBuf,
    package: package::Package,
    shown: Theme,
    metrics: yapfast_iced_kit::theme::Metrics,
    motion: package::Motion,
    animations: Vec<package::Animation>,
    icons: HashMap<String, yapfast_iced_kit::icons::Replacement>,
    sounds: ThemeSounds,
    sound_prefs: SoundPrefs,
    chrome: chrome::Chrome,
    content: content::Content,
    art: ThemeArt,
    layout: theme_layout::Layout,
}

/// How fast theme motion may run given window visibility, focus, Settings
/// coverage, Performance and `motion.unfocused`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pace {
    #[default]
    Full,
    Slow,
    Still,
}

impl Pace {
    pub const SLOW_FPS: u64 = 15;

    pub fn runs(self) -> bool {
        self != Self::Still
    }

    pub fn cap_fps(self, fps: u64) -> u64 {
        match self {
            Self::Full => fps,
            Self::Slow => fps.min(Self::SLOW_FPS),
            Self::Still => 0,
        }
    }
}

pub fn pace_of(
    shown: bool,
    minimized: bool,
    focused: bool,
    covered: bool,
    performance: Performance,
    reduced: bool,
    unfocused: package::Unfocused,
) -> Pace {
    if !shown || minimized || reduced || performance == Performance::Light {
        return Pace::Still;
    }
    if focused && covered {
        return Pace::Slow;
    }
    if focused {
        return Pace::Full;
    }
    match (performance, unfocused) {
        (Performance::Balanced | Performance::Light, _) | (_, package::Unfocused::Still) => {
            Pace::Still
        }
        (Performance::Full, package::Unfocused::Slow) => Pace::Slow,
        (Performance::Full, package::Unfocused::Full) => Pace::Full,
    }
}

/// A person's choices for one installed theme's sound replacements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SoundPrefs {
    pub on: bool,
    pub interface: bool,
    pub volume: u8,
}

impl Default for SoundPrefs {
    fn default() -> Self {
        Self {
            on: true,
            interface: false,
            volume: 50,
        }
    }
}

/// An entry in the installed-theme lock that could not safely be opened.
/// It remains visible in Settings → Themes with the core's exact reason
/// instead of disappearing from the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub id: String,
    pub reason: String,
}

#[derive(Clone, Default)]
pub struct ThemeArt {
    surfaces: HashMap<ArtSurface, Vec<ArtLayer>>,
    overlays: Vec<ArtLayer>,
    chat_overlays: Vec<ArtLayer>,
    procedural_overlays: Vec<art::Overlay>,
    chat_procedural_overlays: Vec<art::Overlay>,
    reserves: HashMap<ArtSurface, art::Reserve>,
    performance: Performance,
    reduced_motion: bool,
    glass: Option<GlassPane>,
    particles: Vec<ThemeParticles>,
    particle_events: Arc<Mutex<Vec<ParticleEvent>>>,
    list_art: HashMap<String, TileArtImage>,
    footer_height: Option<f32>,
    text: BTreeMap<art::TextRole, art::TextLook>,
    events: BTreeMap<art::EventSlot, art::Event>,
    foil: Option<art::Foil>,
}

#[derive(Clone)]
pub struct ThemeParticles {
    pub spec: art::Particles,
    pub sprite: Option<iced::advanced::image::Handle>,
}

#[derive(Clone, Copy)]
pub struct ParticleEvent {
    pub trigger: art::Trigger,
    pub started: Instant,
    pub origin: Option<(f32, f32)>,
}

pub type ParticleView<'a> = (
    &'a [ThemeParticles],
    &'a Arc<Mutex<Vec<ParticleEvent>>>,
    Performance,
    bool,
);

#[derive(Clone)]
pub struct TileArtImage {
    pub handle: iced::advanced::image::Handle,
    pub art: yapfast_core::themes::lists::TileArt,
    pub pieces: Option<Vec<Option<iced::advanced::image::Handle>>>,
    pub frames: Vec<(
        iced::advanced::image::Handle,
        Option<Vec<Option<iced::advanced::image::Handle>>>,
    )>,
}

#[derive(Clone)]
pub struct ArtLayer {
    pub handle: iced::advanced::image::Handle,
    pub frames: Vec<iced::advanced::image::Handle>,
    pub width: u32,
    pub height: u32,
    pub layer: art::Layer,
}

#[derive(Clone)]
pub struct GlassPane {
    pub handle: iced::advanced::image::Handle,
    pub window: (u32, u32),
    pub style: art::Glass,
    grounds: HashMap<ArtSurface, u32>,
}

impl GlassPane {
    pub fn ground(&self, surface: ArtSurface) -> u32 {
        self.grounds.get(&surface).copied().unwrap_or(0xff00_0000)
    }
}

impl ThemeArt {
    fn decorations(&self) -> std::collections::HashSet<String> {
        let mut surfaces = self
            .surfaces
            .keys()
            .map(|surface| surface.name().to_owned())
            .collect::<std::collections::HashSet<_>>();
        if let Some(glass) = &self.glass {
            surfaces.extend(
                glass
                    .style
                    .surfaces
                    .iter()
                    .map(|surface| surface.name().to_owned()),
            );
        }
        surfaces
    }
}

#[derive(Clone, Default)]
struct ThemeSounds {
    app: HashMap<Sound, ThemedSound>,
    rings: HashMap<media::SoundSlot, Arc<[i16]>>,
    cues: HashMap<media::SoundSlot, SoundCue>,
}

#[derive(Clone)]
struct SoundCue {
    samples: Vec<Arc<[i16]>>,
    volume: f32,
}

static NEXT_CUE: AtomicUsize = AtomicUsize::new(0);

/// The themes this data folder offers and the one it chose.
pub struct Themes {
    pub custom: Vec<Custom>,
    pub installed: Vec<Installed>,
    /// Package companions of creator-made custom themes. They use their
    /// `custom:` layout ids but do not appear a second time in Installed.
    companions: Vec<Installed>,
    pub refused: Vec<Refused>,
    pub chosen: Option<String>,
    previous: Option<String>,
    trial: Option<ThemeTrial>,
    creator_preview: Option<Installed>,
    system_mode: Mode,
    window: (u32, u32),
}

const TRIAL_ID: &str = "trial";
const TRIAL_SECONDS: u64 = 30;
pub const CREATOR_PREVIEW_ID: &str = "creator-preview";

struct ThemeTrial {
    id: String,
    name: String,
    back_to: Option<String>,
    until: Instant,
    reviewed: Option<ReviewedTrial>,
}

struct ReviewedTrial {
    fetched: install::Fetched,
    _folder: tempfile::TempDir,
    package: Installed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrialInfo {
    pub name: String,
    pub seconds: u64,
}

struct ResolveSource {
    id: String,
    folder: PathBuf,
    package: package::Package,
    previous: Option<String>,
    cost: String,
}

fn resolve_package(
    source: ResolveSource,
    preferred: Mode,
    layout: &Layout,
    window: (u32, u32),
) -> Result<Installed, String> {
    let ResolveSource {
        id,
        folder,
        package,
        previous,
        cost,
    } = source;
    // Until the mode option is exposed, prefer dark, as the old themes page
    // did when the desktop did not report a light preference.
    let choices = layout.theme_options.get(&id).cloned().unwrap_or_default();
    let mode = selected_mode(&package, &choices, preferred);
    let resolved = package
        .resolve_with(mode, &choices)
        .map_err(|error| error.to_string())?;
    let palette = resolved_palette(&resolved);
    let shown = theme(&id, &palette);
    let mut metrics = metrics(&resolved, layout.theme_performance, layout.reduce_motion);
    metrics.types = typography(&folder, &resolved);
    metrics.readable_text = layout.readable_text;
    let icons = icons(&folder, &resolved, &palette);
    let sounds = sounds(&folder, &resolved, &package.options, &choices);
    let art = theme_art(
        &folder,
        &resolved,
        layout.theme_performance,
        layout.reduce_motion,
        window,
    );
    metrics.decorations = art.decorations();
    metrics.tooltip = tooltip_surface(&art);
    let sound_prefs = layout.theme_sounds.get(&id).copied().unwrap_or_default();
    let cues_on = sound_prefs.interface && sound_prefs.volume > 0;
    metrics.hover_cue = cues_on && sounds.cues.contains_key(&media::SoundSlot::Hover);
    metrics.press_cue = cues_on && sounds.cues.contains_key(&media::SoundSlot::Press);
    shown.register_metrics(metrics.clone());
    let based_on = package.based_on.clone();
    let preview = installed_preview(&folder, &package.preview);
    Ok(Installed {
        id,
        name: package.name.clone(),
        version: package.version.clone(),
        authors: package.authors.clone(),
        description: package.description.clone(),
        modes: package.modes(),
        notes: package.notes.clone(),
        options: package.options.clone(),
        choices,
        previous,
        based_on,
        cost,
        preview,
        palette_preview: palette,
        folder,
        package,
        shown,
        metrics,
        motion: resolved.motion,
        animations: resolved.animations,
        icons,
        sounds,
        sound_prefs,
        chrome: resolved.chrome,
        content: resolved.content,
        art,
        layout: resolved.layout,
    })
}

fn creator_font_bytes(file: &str) -> Option<&'static [u8]> {
    match file {
        "AtkinsonHyperlegibleNext-Regular.ttf" => Some(include_bytes!(
            "../../../ui/fonts/AtkinsonHyperlegibleNext-Regular.ttf"
        )),
        "DisplayName-Tempo.ttf" => Some(include_bytes!("../../../ui/fonts/DisplayName-Tempo.ttf")),
        "DisplayName-Modern.ttf" => {
            Some(include_bytes!("../../../ui/fonts/DisplayName-Modern.ttf"))
        }
        "DisplayName-MonkeyBars.ttf" => Some(include_bytes!(
            "../../../ui/fonts/DisplayName-MonkeyBars.ttf"
        )),
        "DisplayName-Journal.ttf" => {
            Some(include_bytes!("../../../ui/fonts/DisplayName-Journal.ttf"))
        }
        "DisplayName-8Bit.ttf" => Some(include_bytes!("../../../ui/fonts/DisplayName-8Bit.ttf")),
        "DisplayName-Mainframe.ttf" => Some(include_bytes!(
            "../../../ui/fonts/DisplayName-Mainframe.ttf"
        )),
        _ => None,
    }
}

fn regenerate_managed_creator(root: &Path, custom: &Custom, folder: &Path) -> Result<(), String> {
    let Some(spec) = custom.creator.as_ref() else {
        return Ok(());
    };
    let Some(named) = custom.folder.as_deref() else {
        return Ok(());
    };
    let expected = PathBuf::from("themes/creator").join(yapfast_core::themes::creator::slug(
        custom.id.strip_prefix("custom:").unwrap_or(&custom.id),
    ));
    if Path::new(named) != expected || folder != root.join(&expected) || folder.exists() {
        return Ok(());
    }
    // Imported fonts and picture backdrops cannot be recreated from layout
    // metadata alone. Leave those missing folders refused instead of
    // manufacturing a partial package.
    if !spec.fonts.is_empty() || spec.picture.is_some() {
        return Ok(());
    }
    let derived = yapfast_core::themes::creator::derive(spec);
    yapfast_core::themes::creator::write_folder(
        folder,
        spec,
        &custom.name,
        &custom.id,
        &derived,
        creator_font_bytes,
    )
    .map_err(|error| error.to_string())
}

impl Themes {
    fn package(&self, id: &str) -> Option<&Installed> {
        if id == CREATOR_PREVIEW_ID
            && let Some(package) = &self.creator_preview
        {
            return Some(package);
        }
        if id == TRIAL_ID
            && let Some(package) = self
                .trial
                .as_ref()
                .and_then(|trial| trial.reviewed.as_ref())
                .map(|reviewed| &reviewed.package)
        {
            return Some(package);
        }
        self.installed
            .iter()
            .chain(&self.companions)
            .find(|package| package.id == id)
    }

    fn package_mut(&mut self, id: &str) -> Option<&mut Installed> {
        if id == CREATOR_PREVIEW_ID
            && let Some(package) = &mut self.creator_preview
        {
            return Some(package);
        }
        if id == TRIAL_ID
            && let Some(package) = self
                .trial
                .as_mut()
                .and_then(|trial| trial.reviewed.as_mut())
                .map(|reviewed| &mut reviewed.package)
        {
            return Some(package);
        }
        self.installed
            .iter_mut()
            .chain(&mut self.companions)
            .find(|package| package.id == id)
    }

    fn packages_mut(&mut self) -> impl Iterator<Item = &mut Installed> {
        self.installed
            .iter_mut()
            .chain(&mut self.companions)
            .chain(self.trial.iter_mut().filter_map(|trial| {
                trial
                    .reviewed
                    .as_mut()
                    .map(|reviewed| &mut reviewed.package)
            }))
            .chain(self.creator_preview.iter_mut())
    }

    /// The themes `layout` keeps and the packages installed in `root`.
    #[cfg(test)]
    pub fn read(root: &Path, layout: &Layout) -> Self {
        Self::read_sized(root, layout, (1024, 768))
    }

    pub fn read_sized(root: &Path, layout: &Layout, window: (u32, u32)) -> Self {
        let mut custom = layout.custom_themes.clone();
        palette::tidy(&mut custom);
        let (read, refused) = install::Themes::open(root).installed_and_refused();
        let mut refused: Vec<_> = refused
            .into_iter()
            .map(|(id, reason)| Refused { id, reason })
            .collect();
        let installed = read
            .into_iter()
            .filter_map(|installed| {
                let id = format!("{INSTALLED}{}", installed.id);
                let previous = installed
                    .entry
                    .previous
                    .as_ref()
                    .map(|previous| previous.rev.clone());
                let cost = if !installed.entry.cost.is_empty() {
                    installed.entry.cost.clone()
                } else {
                    check::Files::read(&installed.folder)
                        .ok()
                        .map(|files| check::check(&files, Strictness::Install).cost.text())
                        .unwrap_or_default()
                };
                match resolve_package(
                    ResolveSource {
                        id,
                        folder: installed.folder,
                        package: installed.package,
                        previous,
                        cost,
                    },
                    Mode::Dark,
                    layout,
                    window,
                ) {
                    Ok(installed) => Some(installed),
                    Err(error) => {
                        tracing::warn!(
                            id = %installed.id,
                            reason = %error,
                            "an installed theme could not be resolved"
                        );
                        refused.push(Refused {
                            id: installed.id,
                            reason: error,
                        });
                        None
                    }
                }
            })
            .collect();
        let companions = custom
            .iter()
            .filter_map(|custom| {
                let named = custom.folder.as_deref()?;
                let named = PathBuf::from(named);
                let folder = if named.is_absolute() {
                    named
                } else {
                    root.join(named)
                };
                if custom.creator.is_some()
                    && let Err(reason) = regenerate_managed_creator(root, custom, &folder)
                {
                    tracing::warn!(id = %custom.id, %reason, "a missing managed creator theme could not be regenerated");
                }
                let result = check::Files::read(&folder).and_then(|mut files| {
                    // Creator folders already are the complete package
                    // derived from their saved Spec. Older custom folders
                    // are layers only: preserve their type/art/media, but
                    // merge the palette currently edited in layout.json on
                    // top, exactly as the owner's export/apply path does.
                    if custom.creator.is_none() {
                        let colours: Vec<_> = palette::Theme::TOKENS
                            .iter()
                            .enumerate()
                            .filter_map(|(index, token)| {
                                Some((*token, custom.theme.colour(index)?.argb()))
                            })
                            .collect();
                        files.manifest = package::export_with(
                            &custom.name,
                            "",
                            custom.base,
                            custom.theme.dark,
                            &colours,
                            Some(&files.manifest),
                        )?;
                    }
                    let report = check::check(&files, Strictness::Install);
                    if let Some(reason) = report.reason() {
                        return Err(reason.to_owned());
                    }
                    let package = report.package.expect("a passed check has its package");
                    resolve_package(
                        ResolveSource {
                            id: custom.id.clone(),
                            folder,
                            package,
                            previous: None,
                            cost: String::new(),
                        },
                        Mode::Dark,
                        layout,
                        window,
                    )
                });
                match result {
                    Ok(companion) => Some(companion),
                    Err(reason) => {
                        tracing::warn!(id = %custom.id, %reason, "a custom theme package could not be resolved");
                        refused.push(Refused {
                            id: custom.id.clone(),
                            reason,
                        });
                        None
                    }
                }
            })
            .collect();
        Self {
            custom,
            installed,
            companions,
            refused,
            chosen: layout.theme.clone(),
            previous: layout.previous_theme.clone(),
            trial: None,
            creator_preview: None,
            system_mode: Mode::Dark,
            window,
        }
    }

    /// Reads installed packages and resolves them again after their store
    /// changed. The live choice and Back target are deliberately retained:
    /// [`Self::choose`] keeps those directly as well as queuing their
    /// `layout.json` writes, so the `Layout` borrowed by the caller can
    /// briefly still contain the values from before that choice.
    pub fn refresh(&mut self, root: &Path, layout: &Layout) {
        let chosen = self.chosen.clone();
        let previous = self.previous.clone();
        let trial = self.trial.take();
        let creator_preview = self.creator_preview.take();
        let system_mode = self.system_mode;
        let mut refreshed = Self::read_sized(root, layout, self.window);
        refreshed.chosen = chosen;
        refreshed.previous = previous;
        refreshed.trial = trial;
        refreshed.creator_preview = creator_preview;
        *self = refreshed;
        self.system_mode_changed(system_mode);
    }

    /// Re-reads installed packages after their store changes. Creator saves
    /// should call [`Self::refresh`] directly because it also rebuilds custom
    /// package companions.
    pub fn refresh_installed(&mut self, root: &Path, layout: &Layout) {
        self.refresh(root, layout);
    }

    /// Installs checked repository bytes, then re-reads and resolves every
    /// installed package. Choosing the new theme remains the review UI's
    /// explicit next action, as in the owner client.
    pub fn install_fetched(
        &mut self,
        root: &Path,
        layout: &Layout,
        fetched: &install::Fetched,
    ) -> Result<String, String> {
        install::Themes::open(root)
            .install(fetched)
            .map_err(|error| format!("{error:#}"))?;
        self.refresh_installed(root, layout);
        Ok(format!("{INSTALLED}{}", fetched.package().id))
    }

    /// Rolls an installed package back to the version retained by core,
    /// preserving its choice, options, content permission and sound
    /// preferences while resolving that earlier version again.
    ///
    /// `id` is the package id in `theme.json`, without [`INSTALLED`].
    pub fn roll_back(&mut self, root: &Path, layout: &Layout, id: &str) -> Result<(), String> {
        install::Themes::open(root)
            .roll_back(id)
            .map_err(|error| format!("{error:#}"))?;
        self.refresh_installed(root, layout);
        Ok(())
    }

    /// Removes an installed package. Its per-package permissions and
    /// choices go only after core has safely updated its lock. If it was
    /// showing, the owner's previous-theme rule is applied: restore a
    /// different Back target, or show the stock default with no target.
    ///
    /// Returns whether the active choice changed. `id` is the package id in
    /// `theme.json`, without [`INSTALLED`].
    pub fn uninstall(
        &mut self,
        root: &Path,
        layout: &mut Layout,
        id: &str,
    ) -> Result<bool, String> {
        install::Themes::open(root)
            .uninstall(id)
            .map_err(|error| format!("{error:#}"))?;

        let layout_id = format!("{INSTALLED}{id}");
        if self.trial.as_ref().map(|trial| trial.id.as_str()) == Some(layout_id.as_str()) {
            self.trial = None;
        }
        let was_chosen = self.chosen.as_deref() == Some(layout_id.as_str());
        let mut back = None;
        if was_chosen {
            back = self
                .previous
                .clone()
                .filter(|previous| previous != &layout_id);
            // Validate after the package list is refreshed: the old in-memory
            // list still contains the package core just removed.
            self.chosen = None;
            self.previous = None;
        }

        if layout.theme_options.remove(&layout_id).is_some() {
            layout::store(root, "theme_options", &layout.theme_options);
        }
        if layout.theme_sounds.remove(&layout_id).is_some() {
            layout::store(root, "theme_sounds", &layout.theme_sounds);
        }
        if layout.theme_content.remove(&layout_id) {
            layout::store(root, "theme_content", &layout.theme_content);
        }
        self.refresh_installed(root, layout);
        if was_chosen {
            back = back.filter(|id| self.find(id).is_some());
            self.chosen.clone_from(&back);
            layout.theme = back;
            layout.previous_theme = None;
            layout::store(root, "theme", &layout.theme);
            layout::store(root, "previous_theme", &layout.previous_theme);
        }
        Ok(was_chosen)
    }

    /// The theme to show.
    pub fn shown(&self) -> Theme {
        if let Some(theme) = self
            .trial
            .as_ref()
            .and_then(|trial| self.package(&trial.id))
            .map(|package| package.shown.clone())
        {
            return theme;
        }
        if let Some(theme) = self
            .chosen
            .as_deref()
            .and_then(|id| self.package(id).map(|package| package.shown.clone()))
        {
            return theme;
        }
        let (id, found) = palette::chosen(self.chosen.as_deref().or(Some(DEFAULT)), &self.custom);
        theme(id, &found)
    }

    /// The chosen theme's id, as Appearance and Themes mark it.
    pub fn chosen_id(&self) -> &str {
        if let Some(trial) = &self.trial {
            return &trial.id;
        }
        if let Some(installed) = self.chosen.as_deref().and_then(|id| self.package(id)) {
            return &installed.id;
        }
        palette::chosen(self.chosen.as_deref().or(Some(DEFAULT)), &self.custom).0
    }

    /// The theme Settings → Themes' Back action would restore, and its
    /// display name. A missing package or deleted custom theme is not
    /// offered, nor is the theme already shown.
    pub fn previous(&self) -> Option<(&str, &str)> {
        let id = self
            .previous
            .as_deref()
            .filter(|previous| self.chosen.as_deref() != Some(*previous))?;
        let name = self
            .installed
            .iter()
            .find(|installed| installed.id == id)
            .map(|installed| installed.name.as_str())
            .or_else(|| palette::find(id).map(|theme| theme.name))
            .or_else(|| {
                self.custom
                    .iter()
                    .find(|custom| custom.id == id)
                    .map(|custom| custom.name.as_str())
            })?;
        Some((id, name))
    }

    /// `id`'s theme, if it's one Yapfast has or the person made.
    pub fn find(&self, id: &str) -> Option<Theme> {
        self.package(id)
            .map(|package| package.shown.clone())
            .or_else(|| palette::lookup(id, &self.custom).map(|(id, found)| theme(id, &found)))
    }

    /// Resolves a checked creator draft into the ordinary package registry
    /// without choosing or persisting it. All package consumers can read it
    /// through [`CREATOR_PREVIEW_ID`] until [`Self::clear_creator_preview`].
    pub fn preview_creator(&mut self, folder: &Path, layout: &Layout) -> Result<Theme, String> {
        self.clear_creator_preview();
        let files = check::Files::read(folder)?;
        let report = check::check(&files, Strictness::Install);
        if let Some(reason) = report.reason() {
            return Err(reason.to_owned());
        }
        let package = report.package.expect("a passed check has its package");
        let resolved = resolve_package(
            ResolveSource {
                id: CREATOR_PREVIEW_ID.to_owned(),
                folder: folder.to_owned(),
                package,
                previous: None,
                cost: report.cost.text(),
            },
            self.system_mode,
            layout,
            self.window,
        )?;
        let shown = resolved.shown.clone();
        self.creator_preview = Some(resolved);
        Ok(shown)
    }

    pub fn clear_creator_preview(&mut self) {
        self.creator_preview = None;
        Theme::unregister_metrics(CREATOR_PREVIEW_ID);
    }

    /// Removes one custom theme from live theme state and repairs choice
    /// history before its id can be reused. An active deletion restores a
    /// still-valid Back target, or Yap when there is none; the deleted id is
    /// never retained as `previous_theme`.
    pub fn forget_custom(&mut self, root: &Path, id: &str) -> bool {
        self.custom.retain(|custom| custom.id != id);
        self.companions.retain(|companion| companion.id != id);
        Theme::unregister_metrics(id);
        if self.trial.as_ref().is_some_and(|trial| trial.id == id) {
            self.trial = None;
        }

        let active = self.chosen.as_deref() == Some(id);
        if active {
            let fallback = self
                .previous
                .take()
                .filter(|previous| previous != id && self.find(previous).is_some())
                .unwrap_or_else(|| "yap".to_owned());
            self.chosen = Some(fallback);
            layout::store(root, "theme", &self.chosen);
            layout::store(root, "previous_theme", Option::<String>::None);
        } else if self.previous.as_deref() == Some(id) {
            self.previous = None;
            layout::store(root, "previous_theme", Option::<String>::None);
        }
        active
    }

    /// Keeps `id` as the chosen theme in `layout.json`, leaving the rest of
    /// what's kept there as it is.
    pub fn choose(&mut self, root: &Path, id: &str) {
        self.trial = None;
        // Re-choosing the theme already shown must not lose the useful Back
        // target. Every actual change remembers what it replaced, exactly
        // as the owner UI did.
        if self.chosen.as_deref() != Some(id) {
            self.previous = self.chosen.take();
            layout::store(root, "previous_theme", &self.previous);
        }
        self.chosen = Some(id.to_owned());
        layout::store(root, "theme", id);
    }

    /// Turns a package theme off. With no theme kept, the stock default is
    /// shown at once and at the next start; Back can restore the theme that
    /// was showing.
    pub fn turn_off(&mut self, root: &Path) {
        self.trial = None;
        if let Some(shown) = self.chosen.take() {
            self.previous = Some(shown);
            layout::store(root, "previous_theme", &self.previous);
        }
        layout::store(root, "theme", Option::<String>::None);
    }

    /// Restores Settings → Themes' Back target. Going back between two
    /// chosen themes swaps them; restoring after Turn off clears Back, as
    /// in the owner UI.
    pub fn revert(&mut self, root: &Path) -> bool {
        self.trial = None;
        let Some(previous) = self.previous().map(|(id, _)| id.to_owned()) else {
            return false;
        };
        self.choose(root, &previous);
        true
    }

    /// Shows an installed package for 30 seconds without changing
    /// `layout.json`.
    pub fn try_installed(&mut self, id: &str) -> Result<(), String> {
        let package = self
            .installed
            .iter()
            .find(|package| package.id == id)
            .ok_or_else(|| "that installed theme isn't available".to_owned())?;
        let name = package.name.clone();
        let back_to = self
            .trial
            .take()
            .and_then(|trial| trial.back_to)
            .or_else(|| self.chosen.clone());
        self.trial = Some(ThemeTrial {
            id: id.to_owned(),
            name,
            back_to,
            until: Instant::now() + Duration::from_secs(TRIAL_SECONDS),
            reviewed: None,
        });
        Ok(())
    }

    /// Shows checked review bytes for 30 seconds without installing them.
    /// Their files live only in a private temporary folder held by the
    /// trial, so fonts and art remain available until Keep or Revert.
    pub fn try_reviewed(
        &mut self,
        fetched: &install::Fetched,
        layout: &Layout,
    ) -> Result<(), String> {
        let folder = tempfile::Builder::new()
            .prefix(".yapfast-theme-trial-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        std::fs::write(
            folder.path().join(yapfast_core::themes::MANIFEST),
            &fetched.files.manifest,
        )
        .map_err(|error| error.to_string())?;
        for (relative, bytes) in &fetched.files.files {
            let target = folder.path().join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            std::fs::write(target, bytes).map_err(|error| error.to_string())?;
        }
        if let Some((relative, bytes)) = &fetched.files.licence {
            std::fs::write(folder.path().join(relative), bytes)
                .map_err(|error| error.to_string())?;
        }

        let name = fetched.package().name.clone();
        let package = resolve_package(
            ResolveSource {
                id: TRIAL_ID.to_owned(),
                folder: folder.path().to_owned(),
                package: fetched.package().clone(),
                previous: None,
                cost: fetched.report.cost.text(),
            },
            self.system_mode,
            layout,
            self.window,
        )?;
        let back_to = self
            .trial
            .take()
            .and_then(|trial| trial.back_to)
            .or_else(|| self.chosen.clone());
        self.trial = Some(ThemeTrial {
            id: TRIAL_ID.to_owned(),
            name,
            back_to,
            until: Instant::now() + Duration::from_secs(TRIAL_SECONDS),
            reviewed: Some(ReviewedTrial {
                fetched: fetched.clone(),
                _folder: folder,
                package,
            }),
        });
        Ok(())
    }

    pub fn trial(&self) -> Option<TrialInfo> {
        let trial = self.trial.as_ref()?;
        Some(TrialInfo {
            name: trial.name.clone(),
            seconds: trial
                .until
                .saturating_duration_since(Instant::now())
                .as_secs_f32()
                .ceil() as u64,
        })
    }

    /// Keeps the current trial. Reviewed bytes are installed only here;
    /// callers then choose the returned layout id through the normal path.
    pub fn keep_trial(&mut self, root: &Path, layout: &Layout) -> Result<Option<String>, String> {
        let Some(trial) = self.trial.take() else {
            return Ok(None);
        };
        if let Some(reviewed) = &trial.reviewed {
            match self.install_fetched(root, layout, &reviewed.fetched) {
                Ok(id) => Ok(Some(id)),
                Err(error) => {
                    self.trial = Some(trial);
                    Err(error)
                }
            }
        } else {
            Ok(Some(trial.id))
        }
    }

    /// Reverts the current trial and returns the layout id that was showing
    /// before it. `None` inside `Some` means the stock default.
    pub fn revert_trial(&mut self) -> Option<Option<String>> {
        self.trial.take().map(|trial| trial.back_to)
    }

    /// Reverts an expired trial. Settings calls this from its countdown
    /// tick; accepting `now` keeps expiry deterministic in tests.
    pub fn expire_trial(&mut self, now: Instant) -> Option<Option<String>> {
        if self.trial.as_ref().is_some_and(|trial| now >= trial.until) {
            self.revert_trial()
        } else {
            None
        }
    }

    /// Re-resolves live dual-variant packages whose Light or dark choice is
    /// Auto. Explicit Dark/Light choices and the persisted theme id remain
    /// untouched.
    pub fn system_mode_changed(&mut self, mode: Mode) -> bool {
        if self.system_mode == mode {
            return false;
        }
        self.system_mode = mode;
        let window = self.window;
        let mut changed = false;
        for installed in self.packages_mut() {
            if installed.modes.len() < 2
                || matches!(
                    installed
                        .choices
                        .get(options::MODE)
                        .and_then(serde_json::Value::as_str),
                    Some("dark" | "light")
                )
            {
                continue;
            }
            let choices = installed.choices.clone();
            let performance = installed.art.performance;
            let reduced = installed.art.reduced_motion;
            let selected = installed.package.mode_for(mode);
            let Ok(resolved) = installed.package.resolve_with(selected, &choices) else {
                continue;
            };
            apply_resolution(installed, resolved, choices, performance, reduced, window);
            changed = true;
        }
        changed
    }

    /// Rebakes fake-glass backings for the current window pixels. Ordinary
    /// package art is size-independent and is left intact.
    pub fn resized(&mut self, window: (u32, u32)) -> bool {
        if self.window == window {
            return false;
        }
        self.window = window;
        let system_mode = self.system_mode;
        let mut changed = false;
        for installed in self.packages_mut() {
            if installed.art.glass.is_none() {
                continue;
            }
            let mode = selected_mode(&installed.package, &installed.choices, system_mode);
            let Ok(resolved) = installed.package.resolve_with(mode, &installed.choices) else {
                continue;
            };
            let Some(style) = resolved.art.glass.as_ref() else {
                installed.art.glass = None;
                changed = true;
                continue;
            };
            let palette = resolved_palette(&resolved);
            installed.art.glass = Some(glass_pane(
                &installed.folder,
                &resolved,
                &palette,
                style,
                window,
            ));
            installed.metrics.tooltip = tooltip_surface(&installed.art);
            installed.shown.register_metrics(installed.metrics.clone());
            changed = true;
        }
        changed
    }

    /// Reworks each installed theme's popup motion after Themes changes
    /// Performance or Reduce motion, without touching the persisted choice.
    pub fn motion_changed(&mut self, performance: Performance, reduced: bool) {
        for installed in self.packages_mut() {
            installed.metrics.motion = motion(&installed.motion, performance, reduced);
            installed.shown.register_metrics(installed.metrics.clone());
            installed.art.performance = performance;
            installed.art.reduced_motion = reduced;
        }
    }

    /// Applies Appearance's Readable text choice to every package's
    /// companion typography without re-resolving unrelated theme data.
    pub fn readable_changed(&mut self, readable: bool) {
        for installed in self.packages_mut() {
            installed.metrics.readable_text = readable;
            installed.shown.register_metrics(installed.metrics.clone());
        }
    }

    /// The active theme's checked replacement SVGs. Built-in and custom
    /// colour themes have no icon pack.
    pub fn icons(&self, id: &str) -> HashMap<String, yapfast_iced_kit::icons::Replacement> {
        self.package(id)
            .map_or_else(HashMap::new, |installed| installed.icons.clone())
    }

    /// Makes the active package's replacements the ones the core plays.
    /// Built-in and custom colour themes restore Yapfast's own sounds.
    pub fn apply_sounds(&self, id: &str) {
        let installed = self.package(id);
        let sounds = installed.map(|installed| &installed.sounds);
        let notifications = installed.is_some_and(|installed| installed.sound_prefs.on);
        sounds::set_themed(if notifications {
            sounds.map_or_else(HashMap::new, |sounds| sounds.app.clone())
        } else {
            HashMap::new()
        });
        ring::set_themed(
            ring::Tone::Incoming,
            notifications
                .then(|| {
                    sounds.and_then(|sounds| sounds.rings.get(&media::SoundSlot::Ring).cloned())
                })
                .flatten(),
        );
        ring::set_themed(
            ring::Tone::Outgoing,
            notifications
                .then(|| {
                    sounds.and_then(|sounds| sounds.rings.get(&media::SoundSlot::Ringback).cloned())
                })
                .flatten(),
        );
        self.update_cue_sounds(id);
    }

    pub fn sound_prefs(&self, id: &str) -> Option<SoundPrefs> {
        self.package(id).map(|installed| installed.sound_prefs)
    }

    pub fn set_sound_prefs(&mut self, id: &str, prefs: SoundPrefs) -> bool {
        let Some(installed) = self.package_mut(id) else {
            return false;
        };
        installed.sound_prefs = prefs;
        let cues_on = prefs.interface && prefs.volume > 0;
        installed.metrics.hover_cue =
            cues_on && installed.sounds.cues.contains_key(&media::SoundSlot::Hover);
        installed.metrics.press_cue =
            cues_on && installed.sounds.cues.contains_key(&media::SoundSlot::Press);
        installed.shown.register_metrics(installed.metrics.clone());
        true
    }

    pub fn has_sounds(&self, id: &str) -> Option<(bool, bool)> {
        let sounds = &self.package(id)?.sounds;
        Some((
            !sounds.app.is_empty() || !sounds.rings.is_empty(),
            !sounds.cues.is_empty(),
        ))
    }

    /// One active package sound for an interface event. The caller decides
    /// whether the window is focused and which speakers to use.
    pub fn cue(&self, id: &str, slot: media::SoundSlot) -> Option<(Arc<[i16]>, f32)> {
        let cue = self.package(id)?;
        if !cue.sound_prefs.interface {
            return None;
        }
        let volume = cue.sound_prefs.volume.min(100) as f32 / 100.0;
        let cue = cue.sounds.cues.get(&slot)?;
        let index = NEXT_CUE.fetch_add(1, Ordering::Relaxed) % cue.samples.len();
        Some((cue.samples[index].clone(), cue.volume * volume))
    }

    fn update_cue_sounds(&self, id: &str) {
        let installed = self.package(id);
        let (hover, press) =
            match installed.filter(|p| p.sound_prefs.interface && p.sound_prefs.volume > 0) {
                Some(p) => {
                    let scale = p.sound_prefs.volume.min(100) as f32 / 100.0;
                    let pick = |slot| {
                        p.sounds.cues.get(&slot).map(|cue| SoundCue {
                            samples: cue.samples.clone(),
                            volume: cue.volume * scale,
                        })
                    };
                    (pick(media::SoundSlot::Hover), pick(media::SoundSlot::Press))
                }
                None => (None, None),
            };
        if let Ok(mut rt) = cue_runtime().lock() {
            rt.hover = hover;
            rt.press = press;
        }
    }

    pub fn sync_cue_runtime(&self, id: &str, allowed: bool, speaker: Option<String>) {
        if let Ok(mut rt) = cue_runtime().lock() {
            rt.allowed = allowed;
            rt.speaker = speaker;
        }
        self.update_cue_sounds(id);
    }

    pub fn animations(&self, id: &str) -> &[package::Animation] {
        self.package(id)
            .map_or(&[], |installed| installed.animations.as_slice())
    }

    pub fn unfocused(&self, id: &str) -> package::Unfocused {
        self.package(id)
            .and_then(|installed| installed.motion.unfocused)
            .unwrap_or_default()
    }

    pub fn chrome(&self, id: &str) -> Option<&chrome::Chrome> {
        self.package(id).map(|installed| &installed.chrome)
    }

    /// A relabelled piece of chrome, or Wired's exact words when the
    /// active package does not name it.
    pub fn label(
        &self,
        id: &str,
        label: chrome::Label,
        stock: impl Into<String>,
        values: &[(&str, &str)],
    ) -> String {
        self.chrome(id)
            .and_then(|chrome| chrome.labels.get(&label))
            .map_or_else(|| stock.into(), |template| chrome::fill(template, values))
    }

    pub fn marked(&self, id: &str, place: chrome::Place, stock: &str) -> String {
        let Some(chrome) = self.chrome(id) else {
            return stock.to_owned();
        };
        format!(
            "{}{}{}",
            chrome.prefix.get(&place).map_or("", String::as_str),
            stock,
            chrome.suffix.get(&place).map_or("", String::as_str)
        )
    }

    pub fn marked_time(&self, id: &str, stock: &str) -> String {
        self.chrome(id)
            .and_then(|chrome| chrome.time.as_ref())
            .map_or_else(
                || stock.to_owned(),
                |(before, after)| format!("{before}{stock}{after}"),
            )
    }

    pub fn content(&self, id: &str, allowed: bool) -> Option<&content::Content> {
        allowed
            .then(|| self.package(id).map(|package| &package.content))
            .flatten()
    }

    pub fn content_phrase(&self, id: &str) -> Option<String> {
        self.package(id)?.content.phrase()
    }

    pub fn art(&self, id: &str, surface: ArtSurface) -> &[ArtLayer] {
        self.package(id)
            .and_then(|installed| installed.art.surfaces.get(&surface))
            .map_or(&[], Vec::as_slice)
    }

    /// Whether an installed theme places art or glass on this surface.
    /// Callers use this only to make their stock opaque fill transparent so
    /// the shared surface renderer can actually be seen underneath.
    pub fn decorates(&self, id: &str, surface: ArtSurface) -> bool {
        self.package(id).is_some_and(|installed| {
            installed
                .art
                .surfaces
                .get(&surface)
                .is_some_and(|layers| !layers.is_empty())
                || installed
                    .art
                    .glass
                    .as_ref()
                    .is_some_and(|glass| glass.style.surfaces.contains(&surface))
        })
    }

    pub fn overlays(&self, id: &str, chat: bool) -> &[ArtLayer] {
        self.package(id).map_or(&[], |installed| {
            if chat {
                installed.art.chat_overlays.as_slice()
            } else {
                installed.art.overlays.as_slice()
            }
        })
    }

    pub fn procedural_overlays(&self, id: &str, chat: bool) -> &[art::Overlay] {
        self.package(id).map_or(&[], |installed| {
            if chat {
                installed.art.chat_procedural_overlays.as_slice()
            } else {
                installed.art.procedural_overlays.as_slice()
            }
        })
    }

    pub fn art_reserve(&self, id: &str, surface: ArtSurface) -> Option<art::Reserve> {
        self.package(id)?.art.reserves.get(&surface).copied()
    }

    /// Height reserved at the member list's foot for the package's Footer
    /// art. A footer with pictures and no explicit height uses the owner's
    /// 64 px default.
    pub fn footer_height(&self, id: &str) -> Option<f32> {
        self.package(id)?.art.footer_height
    }

    pub fn art_motion(&self, id: &str) -> (Performance, bool) {
        self.package(id)
            .map_or((Performance::Light, false), |installed| {
                (installed.art.performance, installed.art.reduced_motion)
            })
    }

    pub fn glass(&self, id: &str, surface: ArtSurface) -> Option<&GlassPane> {
        let pane = self.package(id)?.art.glass.as_ref()?;
        pane.style.surfaces.contains(&surface).then_some(pane)
    }

    pub fn text_look(&self, id: &str, role: art::TextRole) -> Option<&art::TextLook> {
        self.package(id)?.art.text.get(&role)
    }

    pub fn event_effect(&self, id: &str, slot: art::EventSlot) -> Option<art::Event> {
        self.package(id)?.art.events.get(&slot).copied()
    }

    pub fn foil(&self, id: &str) -> Option<&art::Foil> {
        self.package(id)?.art.foil.as_ref()
    }

    pub fn particles(&self, id: &str) -> Option<ParticleView<'_>> {
        let art = &self.package(id)?.art;
        (!art.particles.is_empty()).then_some((
            art.particles.as_slice(),
            &art.particle_events,
            art.performance,
            art.reduced_motion,
        ))
    }

    pub fn particle_event(&self, id: &str, trigger: art::Trigger, origin: Option<(f32, f32)>) {
        let Some((sets, events, performance, reduced)) = self.particles(id) else {
            return;
        };
        if reduced
            || performance == Performance::Light
            || !sets.iter().any(|set| {
                set.spec.on == trigger && (performance == Performance::Full || !set.spec.heavy)
            })
        {
            return;
        }
        if let Ok(mut events) = events.lock() {
            let now = Instant::now();
            events.retain(|event| now.duration_since(event.started) < Duration::from_secs(12));
            events.push(ParticleEvent {
                trigger,
                started: now,
                origin,
            });
        }
    }

    pub fn list_art(
        &self,
        id: &str,
        tile: &yapfast_core::themes::lists::TileArt,
        surface: ArtSurface,
    ) -> Option<&TileArtImage> {
        self.package(id)?
            .art
            .list_art
            .get(&tile_art_key(tile, surface))
    }

    pub fn package_layout(&self, id: &str) -> Option<&theme_layout::Layout> {
        self.package(id).map(|installed| &installed.layout)
    }

    pub fn channel_rows(&self, id: &str) -> Option<&yapfast_core::themes::lists::ChannelRows> {
        self.package_layout(id)?.channel_rows.as_ref()
    }

    pub fn rail_tiles(&self, id: &str) -> Option<&yapfast_core::themes::lists::RailTiles> {
        self.package_layout(id)?.rail_tiles.as_ref()
    }

    pub fn member_rows(&self, id: &str) -> Option<&yapfast_core::themes::lists::MemberRows> {
        self.package_layout(id)?.member_rows.as_ref()
    }

    /// Changes one installed theme option and resolves the package again.
    /// The normalized choices are returned for the caller to keep in
    /// `layout.json`; an invalid id or value changes nothing.
    pub fn set_option(
        &mut self,
        id: &str,
        option_id: &str,
        value: serde_json::Value,
        performance: Performance,
        reduced: bool,
    ) -> Option<options::Choices> {
        let window = self.window;
        let system_mode = self.system_mode;
        let installed = self.package_mut(id)?;
        let option = installed
            .options
            .iter()
            .find(|option| option.id == option_id)?;
        let mut choices = installed.choices.clone();
        choices.insert(option_id.to_owned(), value);
        let value = option.value(&choices);
        if value == option.default_value() {
            choices.remove(option_id);
        } else {
            choices.insert(option_id.to_owned(), value);
        }
        let mode = selected_mode(&installed.package, &choices, system_mode);
        let resolved = installed.package.resolve_with(mode, &choices).ok()?;
        apply_resolution(
            installed,
            resolved,
            choices.clone(),
            performance,
            reduced,
            window,
        );
        Some(choices)
    }
}

fn apply_resolution(
    installed: &mut Installed,
    resolved: Resolved,
    choices: options::Choices,
    performance: Performance,
    reduced: bool,
    window: (u32, u32),
) {
    let palette = resolved_palette(&resolved);
    let shown = theme(&installed.id, &palette);
    let readable = installed.metrics.readable_text;
    let mut metrics = metrics(&resolved, performance, reduced);
    metrics.types = typography(&installed.folder, &resolved);
    metrics.readable_text = readable;
    let icons = icons(&installed.folder, &resolved, &palette);
    let sounds = sounds(
        &installed.folder,
        &resolved,
        &installed.package.options,
        &choices,
    );
    let art = theme_art(&installed.folder, &resolved, performance, reduced, window);
    metrics.decorations = art.decorations();
    metrics.tooltip = tooltip_surface(&art);
    let cues_on = installed.sound_prefs.interface && installed.sound_prefs.volume > 0;
    metrics.hover_cue = cues_on && sounds.cues.contains_key(&media::SoundSlot::Hover);
    metrics.press_cue = cues_on && sounds.cues.contains_key(&media::SoundSlot::Press);
    shown.register_metrics(metrics.clone());
    installed.shown = shown;
    installed.metrics = metrics;
    installed.palette_preview = palette;
    installed.motion = resolved.motion;
    installed.animations = resolved.animations;
    installed.icons = icons;
    installed.sounds = sounds;
    installed.chrome = resolved.chrome;
    installed.content = resolved.content;
    installed.art = art;
    installed.layout = resolved.layout;
    installed.choices = choices;
}

fn theme_art(
    folder: &Path,
    resolved: &Resolved,
    performance: Performance,
    reduced_motion: bool,
    window: (u32, u32),
) -> ThemeArt {
    let palette = resolved_palette(resolved);
    let mut out = ThemeArt {
        performance,
        reduced_motion,
        text: resolved.art.text.clone(),
        events: resolved.art.events.clone(),
        foil: resolved.art.foil.clone(),
        ..ThemeArt::default()
    };
    for (&surface, layers) in &resolved.art.surfaces {
        let loaded: Vec<_> = layers
            .iter()
            .filter_map(|layer| art_layer(folder, layer, surface_ground(&palette, surface)))
            .collect();
        if !loaded.is_empty() {
            out.surfaces.insert(surface, loaded);
            if surface == ArtSurface::Footer {
                out.footer_height = Some(resolved.art.footer_height.unwrap_or(64.0));
            }
            if let Some(reserve) = resolved.art.reserve(surface) {
                out.reserves.insert(surface, reserve);
            }
        }
    }
    for overlay in &resolved.art.overlays {
        if overlay.heavy && performance == Performance::Light {
            continue;
        }
        if overlay.kind == art::OverlayKind::Image {
            if let Some(layer) = overlay.layer.as_ref()
                && let Some(layer) =
                    art_layer(folder, layer, surface_ground(&palette, ArtSurface::Window))
            {
                if overlay.chat {
                    out.chat_overlays.push(layer);
                } else {
                    out.overlays.push(layer);
                }
            }
        } else if overlay.chat {
            out.chat_procedural_overlays.push(overlay.clone());
        } else {
            out.procedural_overlays.push(overlay.clone());
        }
    }
    out.glass = resolved
        .art
        .glass
        .as_ref()
        .map(|glass| glass_pane(folder, resolved, &palette, glass, window));
    out.particles = resolved
        .art
        .particles
        .iter()
        .map(|spec| ThemeParticles {
            sprite: spec.sprite.as_ref().and_then(|file| {
                let picture = yapfast_core::chat::decode_picture_at(&folder.join(file), 256, 256)?;
                Some(rgba_handle(picture_rgba(&picture)))
            }),
            spec: spec.clone(),
        })
        .collect();
    for (surface, tiles) in [
        (
            ArtSurface::Rail,
            yapfast_core::themes::lists::files(resolved.layout.rail_tiles.as_ref(), None, None),
        ),
        (
            ArtSurface::Sidebar,
            yapfast_core::themes::lists::files(None, resolved.layout.channel_rows.as_ref(), None),
        ),
        (
            ArtSurface::Members,
            yapfast_core::themes::lists::files(None, None, resolved.layout.member_rows.as_ref()),
        ),
    ] {
        let ground = surface_ground(&palette, surface);
        for tile in tiles {
            if let Some(image) = tile_art_image(folder, tile, ground) {
                out.list_art.insert(tile_art_key(tile, surface), image);
            }
        }
    }
    out
}

fn tile_art_key(tile: &yapfast_core::themes::lists::TileArt, surface: ArtSurface) -> String {
    format!("{}:{tile:?}", surface.name())
}

fn tile_art_image(
    folder: &Path,
    tile: &yapfast_core::themes::lists::TileArt,
    ground: u32,
) -> Option<TileArtImage> {
    let picture = baked_art_image(folder, &tile.layer, ground)?;
    let mut frames = Vec::new();
    if let Some(sheet) = tile.layer.frames.filter(|sheet| sheet.count > 1) {
        let columns = sheet.columns.max(1).min(sheet.count);
        let rows = sheet.count.div_ceil(columns);
        let width = (picture.width() / columns).max(1);
        let height = (picture.height() / rows).max(1);
        for index in 0..sheet.count {
            let frame = image::imageops::crop_imm(
                &picture,
                index % columns * width,
                index / columns * height,
                width,
                height,
            )
            .to_image();
            frames.push(tile_art_handles(frame, tile.slice));
        }
    }
    let (handle, pieces) = frames
        .first()
        .cloned()
        .unwrap_or_else(|| tile_art_handles(picture, tile.slice));
    Some(TileArtImage {
        handle,
        art: tile.clone(),
        pieces,
        frames,
    })
}

fn tile_art_handles(
    picture: image::RgbaImage,
    slice: Option<[f32; 4]>,
) -> (
    iced::advanced::image::Handle,
    Option<Vec<Option<iced::advanced::image::Handle>>>,
) {
    let (width, height) = picture.dimensions();
    let pieces = slice.map(|[top, right, bottom, left]| {
        let (left, right, top, bottom) = (
            left.round().clamp(0.0, width as f32) as u32,
            right.round().clamp(0.0, width as f32) as u32,
            top.round().clamp(0.0, height as f32) as u32,
            bottom.round().clamp(0.0, height as f32) as u32,
        );
        let xs = [0, left, width.saturating_sub(right), width];
        let ys = [0, top, height.saturating_sub(bottom), height];
        let mut pieces = vec![None; 9];
        for row in 0..3 {
            for column in 0..3 {
                let piece_width = xs[column + 1].saturating_sub(xs[column]);
                let piece_height = ys[row + 1].saturating_sub(ys[row]);
                if piece_width > 0 && piece_height > 0 {
                    pieces[row * 3 + column] = Some(rgba_handle(
                        image::imageops::crop_imm(
                            &picture,
                            xs[column],
                            ys[row],
                            piece_width,
                            piece_height,
                        )
                        .to_image(),
                    ));
                }
            }
        }
        pieces
    });
    (rgba_handle(picture), pieces)
}

fn art_layer(folder: &Path, layer: &art::Layer, ground: u32) -> Option<ArtLayer> {
    let picture = baked_art_image(folder, layer, ground)?;
    let (mut width, mut height) = picture.dimensions();
    let mut frames = Vec::new();
    if let Some(sheet) = layer.frames.filter(|sheet| sheet.count > 1) {
        let columns = sheet.columns.max(1).min(sheet.count);
        let rows = sheet.count.div_ceil(columns);
        width = (width / columns).max(1);
        height = (height / rows).max(1);
        for index in 0..sheet.count {
            let x = index % columns * width;
            let y = index / columns * height;
            frames.push(rgba_handle(
                image::imageops::crop_imm(&picture, x, y, width, height).to_image(),
            ));
        }
    }
    let handle = frames
        .first()
        .cloned()
        .unwrap_or_else(|| rgba_handle(picture));
    Some(ArtLayer {
        handle,
        frames,
        width,
        height,
        layer: layer.clone(),
    })
}

fn surface_ground(palette: &palette::Theme, surface: ArtSurface) -> u32 {
    surface.fixed_ground().unwrap_or_else(|| {
        palette::token_index(surface.ground())
            .and_then(|index| palette.colour(index))
            .map_or(0xff00_0000, |colour| 0xff00_0000 | colour.rgb_part())
    })
}

fn decoded_art(folder: &Path, layer: &art::Layer) -> Option<image::RgbaImage> {
    match &layer.source {
        Source::Image(file) => {
            let path = folder.join(file);
            let picture = yapfast_core::chat::decode_picture_at(&path, 4096, 4096).or_else(|| {
                tracing::warn!(art = %file.display(), "an installed theme art file could not be decoded");
                None
            })?;
            Some(picture_rgba(&picture))
        }
        Source::Texture(texture) => Some(texture_image(*texture, layer.colour)),
    }
}

fn picture_rgba(picture: &yapfast_core::chat::Picture) -> image::RgbaImage {
    use yapfast_core::dominant_colour::Order;
    let mut rgba = picture.rgba.to_vec();
    if crate::pictures::byte_order(picture) == Order::Bgra {
        for pixel in rgba.chunks_exact_mut(4) {
            let [b, g, r, a] = [pixel[0], pixel[1], pixel[2], pixel[3]];
            let straight = |value: u8| match a {
                0 => 0,
                255 => value,
                _ => ((u16::from(value) * 255 + u16::from(a) / 2) / u16::from(a)).min(255) as u8,
            };
            pixel.copy_from_slice(&[straight(r), straight(g), straight(b), a]);
        }
    }
    image::RgbaImage::from_raw(picture.width, picture.height, rgba)
        .expect("decoded art has its declared dimensions")
}

/// Bakes blur, blend and dim once, as the owner renderer does.
fn baked_art_image(folder: &Path, layer: &art::Layer, ground: u32) -> Option<image::RgbaImage> {
    let mut image = decoded_art(folder, layer)?;
    if layer.fit == art::Fit::Tile && (layer.scale - 1.0).abs() > 0.01 {
        image = image::imageops::resize(
            &image,
            ((image.width() as f32 * layer.scale).round() as u32).clamp(1, 2048),
            ((image.height() as f32 * layer.scale).round() as u32).clamp(1, 2048),
            image::imageops::FilterType::Triangle,
        );
    }
    if layer.blur > 0.0 {
        image = image::imageops::fast_blur(&image, layer.blur / 2.0);
    }
    let base = [
        ((ground >> 16) & 0xff) as u8,
        ((ground >> 8) & 0xff) as u8,
        (ground & 0xff) as u8,
    ];
    for pixel in image.as_mut().chunks_exact_mut(4) {
        for channel in 0..3 {
            let source = f32::from(pixel[channel]) / 255.0;
            let below = f32::from(base[channel]) / 255.0;
            let mixed = match layer.blend {
                art::Blend::Normal => source,
                art::Blend::Multiply => source * below,
                art::Blend::Screen => 1.0 - (1.0 - source) * (1.0 - below),
                art::Blend::Overlay => {
                    if below < 0.5 {
                        2.0 * source * below
                    } else {
                        1.0 - 2.0 * (1.0 - source) * (1.0 - below)
                    }
                }
            };
            pixel[channel] = (mixed * (1.0 - layer.dim) * 255.0).round() as u8;
        }
    }
    Some(image)
}

fn rgba_handle(image: image::RgbaImage) -> iced::advanced::image::Handle {
    let (width, height) = image.dimensions();
    let mut rgba = image.into_raw();
    crate::paint::to_native(&mut rgba);
    iced::advanced::image::Handle::from_rgba(width, height, rgba)
}

fn texture_image(texture: art::Texture, colour: u32) -> image::RgbaImage {
    const SIDE: u32 = 64;
    let [alpha, red, green, blue] = colour.to_be_bytes();
    image::RgbaImage::from_fn(SIDE, SIDE, |x, y| {
        let hash = x
            .wrapping_mul(1_103_515_245)
            .wrapping_add(y.wrapping_mul(12_345))
            .rotate_left((x + y) & 15);
        let coverage = match texture {
            art::Texture::Grain => f32::from((hash & 31) as u8) / 90.0,
            art::Texture::Noise => f32::from((hash & 63) as u8) / 70.0,
            art::Texture::Paper => {
                if y % 13 == 0 || hash % 97 == 0 {
                    0.28
                } else {
                    0.035
                }
            }
            art::Texture::Kraft => {
                if (x + y * 3) % 29 == 0 {
                    0.34
                } else {
                    f32::from((hash & 15) as u8) / 120.0
                }
            }
            art::Texture::Halftone => {
                let (dx, dy) = ((x % 8) as i32 - 4, (y % 8) as i32 - 4);
                if dx * dx + dy * dy <= 4 { 0.65 } else { 0.0 }
            }
        };
        image::Rgba([
            red,
            green,
            blue,
            (coverage.clamp(0.0, 1.0) * f32::from(alpha)).round() as u8,
        ])
    })
}

/// The owner's fake glass: one reduced, preblurred copy of the window art,
/// aligned under every pane. It intentionally never samples other widgets.
fn glass_pane(
    folder: &Path,
    resolved: &Resolved,
    palette: &palette::Theme,
    style: &art::Glass,
    window: (u32, u32),
) -> GlassPane {
    const SHRINK: u32 = yapfast_core::themes::check::art::GLASS_SHRINK;
    let window = (window.0.max(256), window.1.max(192));
    let size = ((window.0 / SHRINK).max(64), (window.1 / SHRINK).max(48));
    let ground = surface_ground(palette, ArtSurface::Window);
    let [_, red, green, blue] = ground.to_be_bytes();
    let mut canvas =
        image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([red, green, blue, 255]));
    for layer in resolved
        .art
        .surfaces
        .get(&ArtSurface::Window)
        .into_iter()
        .flatten()
    {
        let Some(mut picture) = baked_art_image(folder, layer, ground) else {
            continue;
        };
        if let Some(sheet) = layer.frames.filter(|sheet| sheet.count > 1) {
            let columns = sheet.columns.max(1).min(sheet.count);
            let rows = sheet.count.div_ceil(columns);
            picture = image::imageops::crop_imm(
                &picture,
                0,
                0,
                (picture.width() / columns).max(1),
                (picture.height() / rows).max(1),
            )
            .to_image();
        }
        if layer.opacity < 1.0 {
            for pixel in picture.pixels_mut() {
                pixel.0[3] = (f32::from(pixel.0[3]) * layer.opacity).round() as u8;
            }
        }
        let filter = image::imageops::FilterType::Triangle;
        if let Some((width, height)) = layer.size {
            let fitted = image::imageops::resize(
                &picture,
                ((width / SHRINK as f32).round() as u32).max(1),
                ((height / SHRINK as f32).round() as u32).max(1),
                filter,
            );
            let x = (size.0.saturating_sub(fitted.width()) as f32 * layer.position.0) as i64;
            let y = (size.1.saturating_sub(fitted.height()) as f32 * layer.position.1) as i64;
            image::imageops::overlay(&mut canvas, &fitted, x, y);
            continue;
        }
        match layer.fit {
            art::Fit::Tile => {
                let tile = image::imageops::resize(
                    &picture,
                    (picture.width() / SHRINK).max(1),
                    (picture.height() / SHRINK).max(1),
                    filter,
                );
                image::imageops::tile(&mut canvas, &tile);
            }
            art::Fit::Contain => {
                let fitted = image::DynamicImage::ImageRgba8(picture)
                    .resize(size.0, size.1, filter)
                    .to_rgba8();
                image::imageops::overlay(
                    &mut canvas,
                    &fitted,
                    i64::from((size.0 - fitted.width()) / 2),
                    i64::from((size.1 - fitted.height()) / 2),
                );
            }
            art::Fit::Stretch => {
                let fitted = image::imageops::resize(&picture, size.0, size.1, filter);
                image::imageops::overlay(&mut canvas, &fitted, 0, 0);
            }
            art::Fit::Cover => {
                let fitted = image::DynamicImage::ImageRgba8(picture)
                    .resize_to_fill(size.0, size.1, filter)
                    .to_rgba8();
                image::imageops::overlay(&mut canvas, &fitted, 0, 0);
            }
        }
    }
    let canvas = image::imageops::fast_blur(&canvas, (style.blur / SHRINK as f32).max(0.5));
    let grounds = style
        .surfaces
        .iter()
        .copied()
        .map(|surface| (surface, surface_ground(palette, surface)))
        .collect();
    GlassPane {
        handle: rgba_handle(canvas),
        window,
        style: style.clone(),
        grounds,
    }
}

fn sounds(
    folder: &Path,
    resolved: &Resolved,
    package_options: &[options::ThemeOption],
    choices: &options::Choices,
) -> ThemeSounds {
    let enabled = package_options
        .iter()
        .find(|option| option.id == options::SOUNDS)
        .and_then(|option| option.value(choices).as_f64())
        .unwrap_or(100.0)
        .clamp(0.0, 100.0) as f32
        / 100.0;
    if enabled <= 0.0 {
        return ThemeSounds::default();
    }
    let mut out = ThemeSounds::default();
    for (&slot, choice) in &resolved.sounds {
        let samples: Vec<Arc<[i16]>> = choice
            .files
            .iter()
            .filter_map(|file| {
                let path = folder.join(file);
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        tracing::warn!(%error, sound = %file.display(), "an installed theme sound could not be read");
                        return None;
                    }
                };
                match media::decode_sound(&path, &bytes) {
                    Ok(samples) => Some(Arc::from(samples)),
                    Err(error) => {
                        tracing::warn!(%error, sound = %file.display(), "an installed theme sound could not be decoded");
                        None
                    }
                }
            })
            .collect();
        if samples.is_empty() {
            continue;
        }
        let volume = choice.volume * enabled;
        if let Some(sound) = slot.app_sound() {
            out.app.insert(sound, ThemedSound { samples, volume });
        } else if matches!(slot, media::SoundSlot::Ring | media::SoundSlot::Ringback) {
            let scaled: Arc<[i16]> = samples[0]
                .iter()
                .map(|sample| (f32::from(*sample) * volume).round() as i16)
                .collect::<Vec<_>>()
                .into();
            out.rings.insert(slot, scaled);
        } else if slot.interface() {
            out.cues.insert(slot, SoundCue { samples, volume });
        }
    }
    out
}

fn icons(
    folder: &Path,
    resolved: &Resolved,
    palette: &palette::Theme,
) -> HashMap<String, yapfast_iced_kit::icons::Replacement> {
    resolved
        .icons
        .iter()
        .filter_map(|(name, choice)| {
            let svg = match std::fs::read_to_string(folder.join(&choice.file)) {
                Ok(svg) => svg,
                Err(error) => {
                    tracing::warn!(%error, icon = *name, "an installed theme icon could not be read");
                    return None;
                }
            };
            let colour = match choice.colour {
                media::IconColour::Tint => yapfast_iced_kit::icons::ReplacementColour::Tint,
                media::IconColour::Keep => yapfast_iced_kit::icons::ReplacementColour::Keep,
                media::IconColour::Colour(colour) => {
                    yapfast_iced_kit::icons::ReplacementColour::Colour(colour & 0x00ff_ffff)
                }
                media::IconColour::Token(token) => {
                    let colour = palette::token_index(token)
                        .and_then(|index| palette.colour(index))
                        .map_or(0, Colour::rgb_part);
                    yapfast_iced_kit::icons::ReplacementColour::Colour(colour)
                }
            };
            Some((
                (*name).to_owned(),
                yapfast_iced_kit::icons::Replacement {
                    svg: svg.into(),
                    colour,
                },
            ))
        })
        .collect()
}

/// The package's variant after its built-in mode option. `auto` keeps the
/// existing dark preference until iced exposes a stable system-theme value
/// at this boundary.
fn selected_mode(
    package: &package::Package,
    choices: &options::Choices,
    system_mode: Mode,
) -> Mode {
    let preferred = match choices
        .get(options::MODE)
        .and_then(serde_json::Value::as_str)
    {
        Some("light") => Mode::Light,
        Some("dark") => Mode::Dark,
        _ => system_mode,
    };
    package.mode_for(preferred)
}

/// A resolved package's shape, spacing and motion tokens in the kit's
/// shape-preserving companion data.
fn metrics(
    resolved: &Resolved,
    performance: Performance,
    reduced: bool,
) -> yapfast_iced_kit::theme::Metrics {
    let paints = resolved
        .colours
        .iter()
        .filter(|(token, _)| {
            package::BRUSHES.contains(token) || package::EXTRA_COLOURS.contains(token)
        })
        .map(|(token, value)| ((*token).to_owned(), paint(value)))
        .collect();
    let mut decorations: std::collections::HashSet<String> = resolved
        .art
        .surfaces
        .iter()
        .filter(|(_, layers)| !layers.is_empty())
        .map(|(surface, _)| surface.name().to_owned())
        .collect();
    if let Some(glass) = &resolved.art.glass {
        decorations.extend(
            glass
                .surfaces
                .iter()
                .map(|surface| surface.name().to_owned()),
        );
    }
    let shapes = resolved
        .shapes
        .iter()
        .map(|(surface, shape)| {
            (
                surface.name().to_owned(),
                yapfast_iced_kit::theme::Shape {
                    radius: shape.radius,
                    border: shape.border,
                    border_colour: shape.border_colour.map(argb),
                    shadow: shape.shadow.map(shadow),
                    inset: shape.inset,
                    inset_top: shape.inset_top,
                    width: shape.width,
                    hover_width: shape.hover_width,
                    ground: shape.ground.as_ref().map(paint),
                    text: shape.text.map(argb),
                    bar: shape.bar.as_ref().map(paint),
                    bar_text: shape.bar_text.map(argb),
                    stripe: shape.stripe.map(|stripe| yapfast_iced_kit::theme::Stripe {
                        colour: argb(stripe.colour),
                        channels: stripe.channels,
                        members: stripe.members,
                        messages: stripe.messages,
                    }),
                    case: shape.case,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let spacing = &resolved.spacing;
    let outlines = resolved
        .layout
        .borders
        .iter()
        .map(|(place, border)| {
            let stroke = match border.stroke {
                yapfast_core::themes::layout::Stroke::Solid => {
                    yapfast_iced_kit::theme::OutlineStroke::Solid
                }
                yapfast_core::themes::layout::Stroke::Dashed => {
                    yapfast_iced_kit::theme::OutlineStroke::Dashed
                }
                yapfast_core::themes::layout::Stroke::Dotted => {
                    yapfast_iced_kit::theme::OutlineStroke::Dotted
                }
            };
            (
                place.name().to_owned(),
                yapfast_iced_kit::theme::Outline {
                    stroke,
                    dash: border.dash,
                    gap: border.gap,
                    width: border.width,
                    colour: border.colour.map(argb),
                },
            )
        })
        .collect();
    yapfast_iced_kit::theme::Metrics {
        paints,
        decorations,
        shapes,
        outlines,
        radii: resolved.radii,
        shadows: resolved.shadows.map(|shadow| shadow.map(self::shadow)),
        outline_width: resolved.layout.outline_width,
        spacing: yapfast_iced_kit::theme::Spacing {
            message_gap: spacing.message_gap,
            message_group_gap: spacing.message_group_gap,
            channel_row_height: resolved
                .layout
                .channel_rows
                .as_ref()
                .and_then(|rows| rows.height)
                .or(spacing.channel_row_height),
            member_row_height: resolved
                .layout
                .member_rows
                .as_ref()
                .and_then(|rows| rows.height)
                .or(spacing.member_row_height),
            avatar_size: spacing.avatar_size,
            chat_padding: spacing.chat_padding,
            sidebar_width: spacing.sidebar_width,
            members_width: spacing.members_width,
        },
        motion: motion(&resolved.motion, performance, reduced),
        types: HashMap::new(),
        readable_text: false,
        tooltip: None,
        hover_cue: false,
        press_cue: false,
    }
}

/// Loads each resolved owner-theme font and keeps all typography tokens at
/// the kit Theme boundary. A file that somehow becomes unreadable after the
/// package was checked falls back to Wired's face for that role, while its
/// remaining size/weight/case tokens still apply.
fn typography(
    folder: &Path,
    resolved: &Resolved,
) -> HashMap<String, yapfast_iced_kit::theme::TypeStyle> {
    resolved
        .types
        .iter()
        .map(|(role, kind)| {
            let font = kind.font.as_ref().and_then(|file| {
                let path = folder.join(file);
                match std::fs::read(&path) {
                    Ok(bytes) => match yapfast_iced_kit::fonts::load_theme_font(bytes) {
                        Ok(font) => Some(font.font),
                        Err(error) => {
                            tracing::warn!(%error, font = %path.display(), "an installed theme font could not be loaded");
                            None
                        }
                    },
                    Err(error) => {
                        tracing::warn!(%error, font = %path.display(), "an installed theme font could not be read");
                        None
                    }
                }
            });
            (
                role.name().to_owned(),
                yapfast_iced_kit::theme::TypeStyle {
                    font,
                    size: kind.size,
                    weight: kind.weight,
                    letter_spacing: kind.letter_spacing,
                    uppercase: kind.uppercase,
                    header_size: kind.header_size,
                    title_size: kind.title_size,
                },
            )
        })
        .collect()
}

fn shadow(shadow: package::Shadow) -> yapfast_iced_kit::theme::ShapeShadow {
    yapfast_iced_kit::theme::ShapeShadow {
        blur: shadow.blur,
        x: shadow.x,
        y: shadow.y,
        colour: argb(shadow.colour),
    }
}

pub(crate) fn paint(paint: &package::Paint) -> iced::Background {
    match paint {
        package::Paint::Colour(colour) => argb(*colour).into(),
        package::Paint::Linear { angle, stops } => {
            let mut gradient = iced::gradient::Linear::new(iced::Degrees(*angle));
            for (colour, offset) in stops.iter().take(8) {
                gradient = gradient.add_stop(offset.clamp(0.0, 1.0), argb(*colour));
            }
            gradient.into()
        }
    }
}

fn motion(
    motion: &package::Motion,
    performance: Performance,
    reduced: bool,
) -> yapfast_iced_kit::theme::Motion {
    use yapfast_iced_kit::theme::{Close, Curve, Hover, Press, Transition};
    let still = performance == Performance::Light || reduced;
    let millis = |value: Option<f32>, stock: f32| {
        Duration::from_micros((value.filter(|_| !still).unwrap_or(stock) * 1000.0).round() as u64)
    };
    let quick = millis(motion.quick, 120.0);
    let curve = match motion.curve.filter(|_| !still).unwrap_or_default() {
        package::Curve::Linear => Curve::Linear,
        package::Curve::Ease => Curve::Ease,
        package::Curve::EaseIn => Curve::EaseIn,
        package::Curve::EaseOut => Curve::EaseOut,
        package::Curve::EaseInOut => Curve::EaseInOut,
        package::Curve::Spring => Curve::Spring,
        package::Curve::Bounce => Curve::Bounce,
        package::Curve::Elastic => Curve::Elastic,
    };
    let open = match motion.open.filter(|_| !still).unwrap_or_default() {
        package::Transition::Grow => Transition::Grow,
        package::Transition::None => Transition::None,
        package::Transition::Fade => Transition::Fade,
        package::Transition::Zoom => Transition::Zoom,
        package::Transition::Pop => Transition::Pop,
    };
    let close = match motion.close.filter(|_| !still).unwrap_or_default() {
        package::Close::Reverse => Close::Reverse,
        package::Close::None => Close::None,
    };
    let hover = match motion.hover.filter(|_| !still).unwrap_or_default() {
        package::Hover::Fade => Hover::Fade,
        package::Hover::None => Hover::None,
        package::Hover::Lift => Hover::Lift,
        package::Hover::Glow => Hover::Glow,
        package::Hover::Scale => Hover::Scale,
    };
    let press = match motion.press.filter(|_| !still).unwrap_or_default() {
        package::Press::None => Press::None,
        package::Press::Scale => Press::Scale,
        package::Press::Sink => Press::Sink,
    };
    let unfocused = match motion.unfocused.unwrap_or_default() {
        package::Unfocused::Full => yapfast_iced_kit::theme::Unfocused::Full,
        package::Unfocused::Slow => yapfast_iced_kit::theme::Unfocused::Slow,
        package::Unfocused::Still => yapfast_iced_kit::theme::Unfocused::Still,
    };
    yapfast_iced_kit::theme::Motion {
        quick,
        popup: millis(motion.popup, 100.0),
        hover_in: motion
            .hover_in
            .filter(|_| !still)
            .map_or(quick, |ms| millis(Some(ms), 120.0)),
        hover_out: motion
            .hover_out
            .filter(|_| !still)
            .map_or(quick, |ms| millis(Some(ms), 120.0)),
        curve,
        hover,
        press,
        open,
        close,
        unfocused,
    }
}

fn installed_preview(folder: &Path, preview: &[PathBuf]) -> Option<iced::advanced::image::Handle> {
    preview.iter().find_map(|rel| {
        let bytes = std::fs::read(folder.join(rel)).ok()?;
        crate::theme_remote::decode_preview(&bytes).map(|p| p.handle)
    })
}

fn tooltip_surface(art: &ThemeArt) -> Option<yapfast_iced_kit::theme::TooltipSurface> {
    use yapfast_iced_kit::theme::{TooltipFit, TooltipGlass, TooltipLayer, TooltipSurface};
    let mut layers = Vec::new();
    if let Some(surface_layers) = art.surfaces.get(&ArtSurface::Tooltip) {
        for item in surface_layers {
            let layer = &item.layer;
            let period_ms = layer
                .frames
                .filter(|f| f.count > 1)
                .map_or(0.0, |f| f.period.max(100.0));
            let fit = match layer.fit {
                art::Fit::Cover => TooltipFit::Cover,
                art::Fit::Contain => TooltipFit::Contain,
                art::Fit::Tile => TooltipFit::Tile,
                art::Fit::Stretch => TooltipFit::Stretch,
            };
            layers.push(TooltipLayer {
                handle: item.handle.clone(),
                frames: item.frames.clone(),
                period_ms,
                width: item.width,
                height: item.height,
                opacity: layer.opacity,
                scale: layer.scale,
                position: layer.position,
                fit,
                drift: layer.drift,
                size: layer.size,
                moves: !art.reduced_motion
                    && art.performance != Performance::Light
                    && (art.performance == Performance::Full || !layer.heavy)
                    && (layer.frames.is_some_and(|f| f.count > 1)
                        || layer.drift.0 != 0.0
                        || layer.drift.1 != 0.0),
            });
        }
    }
    let glass = art.glass.as_ref().and_then(|pane| {
        if !pane.style.surfaces.contains(&ArtSurface::Tooltip) {
            return None;
        }
        let s = &pane.style;
        Some(TooltipGlass {
            handle: pane.handle.clone(),
            window: pane.window,
            ground: argb(pane.ground(ArtSurface::Tooltip)),
            opacity: s.opacity,
            radius: s.radius,
            rim: s.rim.map(|(c, w)| (argb(c), w)),
            highlight: s.highlight.map(argb),
            inset: s
                .inset
                .as_ref()
                .map(|shadow| (argb(shadow.colour), shadow.blur)),
            gap: s.gap,
        })
    });
    if layers.is_empty() && glass.is_none() {
        return None;
    }
    let reserve = art
        .reserves
        .get(&ArtSurface::Tooltip)
        .map(|r| [r.top, r.right, r.bottom, r.left]);
    Some(TooltipSurface {
        layers,
        glass,
        reserve,
    })
}

const HOVER_GAP: Duration = Duration::from_millis(70);

#[derive(Default)]
struct CueRuntime {
    hover: Option<SoundCue>,
    press: Option<SoundCue>,
    speaker: Option<String>,
    allowed: bool,
    last_hover: Option<Instant>,
}

static CUE_RUNTIME: std::sync::OnceLock<Mutex<CueRuntime>> = std::sync::OnceLock::new();

fn cue_runtime() -> &'static Mutex<CueRuntime> {
    CUE_RUNTIME.get_or_init(|| Mutex::new(CueRuntime::default()))
}

pub fn register_cue_handler() {
    yapfast_iced_kit::theme::set_cue_handler(|event| {
        let (samples, volume, speaker) = {
            let Ok(mut rt) = cue_runtime().lock() else {
                return;
            };
            if !rt.allowed {
                return;
            }
            let now = Instant::now();
            let cue = match event {
                yapfast_iced_kit::theme::CueEvent::Hover => {
                    if rt
                        .last_hover
                        .is_some_and(|last| now.saturating_duration_since(last) < HOVER_GAP)
                    {
                        return;
                    }
                    let cue = rt.hover.as_ref().map(|c| {
                        let idx = NEXT_CUE.fetch_add(1, Ordering::Relaxed) % c.samples.len();
                        (c.samples[idx].clone(), c.volume)
                    });
                    if cue.is_some() {
                        rt.last_hover = Some(now);
                    }
                    cue
                }
                yapfast_iced_kit::theme::CueEvent::Press => rt.press.as_ref().map(|c| {
                    let idx = NEXT_CUE.fetch_add(1, Ordering::Relaxed) % c.samples.len();
                    (c.samples[idx].clone(), c.volume)
                }),
            };
            let Some((samples, volume)) = cue else {
                return;
            };
            (samples, volume, rt.speaker.clone())
        };
        yapfast_core::sounds::play_decoded(
            samples,
            volume * yapfast_core::volume::speakers(),
            speaker.as_deref(),
        );
    });
}

pub(crate) fn argb(colour: u32) -> Color {
    Color::from_rgba8(
        (colour >> 16) as u8,
        (colour >> 8) as u8,
        colour as u8,
        ((colour >> 24) as u8) as f32 / 255.0,
    )
}

pub(crate) fn list_case(
    case: Option<yapfast_core::themes::lists::Case>,
    words: &str,
) -> Cow<'_, str> {
    use yapfast_core::themes::lists::Case;
    match case {
        Some(Case::Upper) => Cow::Owned(words.to_uppercase()),
        Some(Case::Lower) => Cow::Owned(words.to_lowercase()),
        _ => Cow::Borrowed(words),
    }
}

pub(crate) fn list_font(
    theme: &Theme,
    role: Option<yapfast_core::themes::lists::FontRole>,
    fallback_role: &'static str,
    weight: Option<f32>,
    stock: iced::Font,
) -> iced::Font {
    let role = role.map_or(fallback_role, yapfast_core::themes::lists::FontRole::name);
    let weighted = match weight.map(|w| w.round() as u16) {
        Some(w) if w >= 650 => crate::fonts::BOLD,
        Some(w) if w >= 550 => crate::fonts::SEMIBOLD,
        Some(w) if w >= 450 => crate::fonts::MEDIUM,
        Some(_) => crate::fonts::REGULAR,
        None => stock,
    };
    theme.font(role, weighted)
}

pub(crate) fn offset_shadow(shadow: Option<(u32, f32, f32)>) -> crate::piece::TextLight {
    crate::piece::TextLight {
        shadow: shadow.map(|(colour, x, y)| (argb(colour), iced::Vector::new(x, y))),
        ..Default::default()
    }
}

/// A resolved package's colours over the built-in base it names.
fn resolved_palette(resolved: &Resolved) -> palette::Theme {
    let mut colours = palette::find(&resolved.base).map_or(palette::YAP, |base| base.theme);
    colours.dark = resolved.dark;
    for (token, paint) in &resolved.colours {
        if let Some(slot) = palette::token_index(token).and_then(|index| colours.colour_mut(index))
        {
            *slot = Colour::from_argb(paint.solid());
        }
    }
    colours
}

/// Fills the kit's theme from the core's.
macro_rules! from_palette {
    ($($name:ident,)*) => {
        /// The core's `theme`, named `id`, as the window paints with it.
        pub fn theme(id: &str, theme: &palette::Theme) -> Theme {
            Theme::unregister_metrics(id);
            let mut shown = Theme::new(id, theme.dark);
            $(shown.$name = color(theme.$name);)*
            shown.mention = color(theme.mention());
            shown.worked_out()
        }
    };
}

yapfast_iced_kit::for_theme_colours!(from_palette);

/// A core colour as iced's.
pub fn color(colour: Colour) -> Color {
    Color::from_rgba8(
        colour.red(),
        colour.green(),
        colour.blue(),
        f32::from(colour.alpha()) / 255.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LAYOUT;

    fn fetched_package(version: &str, chat: &str) -> install::Fetched {
        install::Fetched::new(
            "https://github.com/ada/plum".into(),
            String::new(),
            "main".into(),
            format!("{version}-commit"),
            yapfast_core::themes::check::Files {
                manifest: format!(
                    r##"{{ "theme_api": 1, "id": "com.example.plum", "name": "Plum", "version": "{version}",
                    "variants": {{ "dark": {{ "base": "yap", "colours": {{ "chat": "{chat}" }} }} }} }}"##
                ),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn fetched_package_with_option() -> install::Fetched {
        install::Fetched::new(
            "https://github.com/ada/plum".into(),
            String::new(),
            "main".into(),
            "options-commit".into(),
            yapfast_core::themes::check::Files {
                manifest: r##"{
                    "theme_api": 1,
                    "id": "com.example.plum",
                    "name": "Plum",
                    "version": "1.0.0",
                    "options": [{
                        "id": "wide",
                        "label": "Wide rail",
                        "kind": "toggle",
                        "default": false,
                        "on": { "layout.rail": "right" }
                    }],
                    "variants": { "dark": { "base": "yap" } }
                }"##
                .into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn fetched_dual_package() -> install::Fetched {
        install::Fetched::new(
            "https://github.com/ada/plum".into(),
            String::new(),
            "main".into(),
            "dual-commit".into(),
            yapfast_core::themes::check::Files {
                manifest: r##"{
                    "theme_api": 1,
                    "id": "com.example.plum",
                    "name": "Plum",
                    "version": "1.0.0",
                    "variants": {
                        "dark": { "base": "yap" },
                        "light": { "base": "sakura" }
                    }
                }"##
                .into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn creator_package_companion_survives_layout_restart() {
        use yapfast_core::themes::creator::{self, Messages, Spec};

        let data = tempfile::tempdir().unwrap();
        let relative = "themes/creator/custom-1";
        let folder = data.path().join(relative);
        let spec = Spec {
            body: "slab".into(),
            corners: 90,
            messages: Messages::Cards,
            glass: true,
            ..Spec::default()
        };
        let derived = creator::derive(&spec);
        creator::write_folder(
            &folder,
            &spec,
            "Companion",
            "custom:1",
            &derived,
            |file| match file {
                "DisplayName-Tempo.ttf" => {
                    Some(include_bytes!("../../../ui/fonts/DisplayName-Tempo.ttf"))
                }
                _ => None,
            },
        )
        .unwrap();
        let custom = Custom {
            id: "custom:1".into(),
            name: "Companion".into(),
            base: "yap",
            theme: palette::YAP,
            creator: Some(spec),
            folder: Some(relative.into()),
        };
        layout::store(data.path(), "custom_themes", [&custom]);
        layout::store(data.path(), "theme", "custom:1");
        layout::flush(data.path());

        let kept = Layout::read(data.path());
        let mut themes = Themes::read(data.path(), &kept);
        assert_eq!(themes.chosen_id(), "custom:1");
        assert_eq!(themes.companions.len(), 1, "{:?}", themes.refused);
        assert_eq!(themes.companions[0].folder, folder);
        assert_ne!(
            themes.shown().surface_radius("button", 6.0),
            theme("stock", &palette::YAP).surface_radius("button", 6.0),
            "the creator package's shape, not only its palette, reloads"
        );
        assert!(
            themes.companions[0]
                .metrics
                .types
                .get("body")
                .and_then(|style| style.font)
                .is_some(),
            "the creator package's checked font role reloads"
        );
        assert!(
            themes.companions[0].art.glass.is_some(),
            "the creator package's glass art reloads"
        );
        assert!(themes.resized((1440, 900)));
        assert_eq!(
            themes.companions[0].art.glass.as_ref().unwrap().window,
            (1440, 900),
            "glass backing follows the live window size"
        );

        // Deleting that creator and later reusing its custom id for a plain
        // palette must not inherit the package's global companion metrics.
        drop(themes);
        let palette_only = Custom {
            id: "custom:1".into(),
            name: "Palette only".into(),
            base: "yap",
            theme: palette::YAP,
            creator: None,
            folder: None,
        };
        let plain_layout = Layout {
            theme: Some("custom:1".into()),
            custom_themes: vec![palette_only],
            ..Layout::default()
        };
        let plain = Themes::read(data.path(), &plain_layout).shown();
        assert_eq!(plain.surface_radius("button", 6.0), 6.0);
    }

    #[test]
    fn an_arbitrary_custom_folder_layers_over_its_layout_palette() {
        use yapfast_core::themes::creator::{self, Spec};

        let data = tempfile::tempdir().unwrap();
        let relative = "themes/custom/layered";
        let folder = data.path().join(relative);
        let spec = Spec {
            accent: 30,
            corners: 90,
            ..Spec::default()
        };
        creator::write_folder(
            &folder,
            &spec,
            "Layered",
            "custom:1",
            &creator::derive(&spec),
            |_| None,
        )
        .unwrap();

        let mut colours = palette::YAP;
        colours.avatar = Colour::rgb(0x123456);
        let custom = Custom {
            id: "custom:1".into(),
            name: "Layered".into(),
            base: "yap",
            theme: colours,
            creator: None,
            folder: Some(relative.into()),
        };
        let layout = Layout {
            theme: Some(custom.id.clone()),
            custom_themes: vec![custom],
            ..Layout::default()
        };

        let themes = Themes::read(data.path(), &layout);
        assert_eq!(themes.companions.len(), 1, "{:?}", themes.refused);
        assert_eq!(themes.shown().avatar, color(Colour::rgb(0x123456)));
        assert_ne!(
            themes.shown().surface_radius("button", 6.0),
            theme("stock", &colours).surface_radius("button", 6.0),
            "the linked folder's non-colour layers remain active"
        );
    }

    #[test]
    fn a_missing_managed_creator_folder_regenerates_from_its_saved_spec() {
        let data = tempfile::tempdir().unwrap();
        let relative = "themes/creator/1";
        let custom = Custom {
            id: "custom:1".into(),
            name: "Recovered".into(),
            base: "yap",
            theme: palette::YAP,
            creator: Some(yapfast_core::themes::creator::Spec {
                body: "slab".into(),
                glass: true,
                ..Default::default()
            }),
            folder: Some(relative.into()),
        };
        let layout = Layout {
            theme: Some(custom.id.clone()),
            custom_themes: vec![custom],
            ..Layout::default()
        };
        assert!(!data.path().join(relative).exists());
        let themes = Themes::read(data.path(), &layout);
        assert!(data.path().join(relative).join("theme.json").is_file());
        assert_eq!(themes.companions.len(), 1);
        assert_eq!(themes.chosen_id(), "custom:1");
    }

    #[test]
    fn forgetting_an_active_custom_never_keeps_its_deleted_id() {
        let data = tempfile::tempdir().unwrap();
        let custom = Custom {
            id: "custom:1".into(),
            name: "Gone".into(),
            base: "yap",
            theme: palette::YAP,
            creator: None,
            folder: None,
        };
        let layout = Layout {
            theme: Some("custom:1".into()),
            previous_theme: Some("sakura".into()),
            custom_themes: vec![custom],
            ..Layout::default()
        };
        let mut themes = Themes::read(data.path(), &layout);
        assert!(themes.forget_custom(data.path(), "custom:1"));
        assert!(themes.custom.is_empty());
        assert_eq!(themes.chosen.as_deref(), Some("sakura"));
        assert!(themes.previous.is_none());

        themes.chosen = Some("custom:missing".into());
        themes.previous = Some("also-missing".into());
        assert!(themes.forget_custom(data.path(), "custom:missing"));
        assert_eq!(themes.chosen.as_deref(), Some("yap"));
        assert!(themes.previous.is_none());
    }

    #[test]
    fn package_lifecycle_refreshes_without_losing_preferences() {
        let folder = tempfile::tempdir().unwrap();
        let layout_id = "installed:com.example.plum";
        std::fs::write(
            folder.path().join(LAYOUT),
            format!(
                r#"{{
                    "theme": "sakura",
                    "previous_theme": "yap",
                    "theme_options": {{ "{layout_id}": {{ "kept": true }} }},
                    "theme_content": ["{layout_id}"],
                    "theme_sounds": {{ "{layout_id}": {{ "on": false, "interface": true, "volume": 37 }} }},
                    "zoom": 1.2
                }}"#
            ),
        )
        .unwrap();
        let mut layout = Layout::read(folder.path());
        let mut themes = Themes::read(folder.path(), &layout);

        assert_eq!(
            themes
                .install_fetched(folder.path(), &layout, &fetched_package("1.0.0", "#2a1a3a"))
                .unwrap(),
            layout_id
        );
        assert_eq!(themes.installed[0].version, "1.0.0");
        assert_eq!(themes.installed[0].choices["kept"], true);
        assert_eq!(
            themes.sound_prefs(layout_id),
            Some(SoundPrefs {
                on: false,
                interface: true,
                volume: 37,
            })
        );
        assert!(layout.theme_content.contains(layout_id));

        let error = themes
            .roll_back(folder.path(), &layout, "com.example.plum")
            .unwrap_err();
        assert_eq!(error, "there's no earlier version to go back to");
        themes.choose(folder.path(), layout_id);
        themes
            .install_fetched(folder.path(), &layout, &fetched_package("1.1.0", "#2b1b3b"))
            .unwrap();
        assert_eq!(themes.installed[0].version, "1.1.0");
        assert_eq!(
            themes.installed[0].previous.as_deref(),
            Some("1.0.0-commit")
        );
        assert_eq!(themes.chosen.as_deref(), Some(layout_id));
        assert_eq!(themes.previous.as_deref(), Some("sakura"));
        themes
            .roll_back(folder.path(), &layout, "com.example.plum")
            .unwrap();
        assert_eq!(themes.installed[0].version, "1.0.0");
        assert!(themes.installed[0].previous.is_none());
        assert_eq!(themes.installed[0].choices["kept"], true);
        assert!(layout.theme_content.contains(layout_id));

        assert!(
            themes
                .uninstall(folder.path(), &mut layout, "com.example.plum")
                .unwrap()
        );
        assert!(themes.installed.is_empty());
        assert_eq!(themes.chosen.as_deref(), Some("sakura"));
        assert!(themes.previous.is_none());
        assert_eq!(layout.theme.as_deref(), Some("sakura"));
        assert!(layout.previous_theme.is_none());
        assert!(!layout.theme_options.contains_key(layout_id));
        assert!(!layout.theme_sounds.contains_key(layout_id));
        assert!(!layout.theme_content.contains(layout_id));

        layout::flush(folder.path());
        let kept = Layout::read(folder.path());
        assert_eq!(kept.theme.as_deref(), Some("sakura"));
        assert!(kept.previous_theme.is_none());
        assert_eq!(kept.zoom, Some(1.2), "unrelated layout keys stay");
        assert!(!kept.theme_options.contains_key(layout_id));
        assert!(!kept.theme_sounds.contains_key(layout_id));
        assert!(!kept.theme_content.contains(layout_id));
    }

    #[test]
    fn uninstall_does_not_restore_a_missing_back_target() {
        let data = tempfile::tempdir().unwrap();
        let mut layout = Layout {
            theme: Some("installed:com.example.plum".into()),
            previous_theme: Some("installed:gone".into()),
            ..Layout::default()
        };
        let mut themes = Themes::read(data.path(), &layout);
        themes
            .install_fetched(data.path(), &layout, &fetched_package("1.0.0", "#2a1a3a"))
            .unwrap();
        assert!(
            themes
                .uninstall(data.path(), &mut layout, "com.example.plum")
                .unwrap()
        );
        assert!(themes.chosen.is_none());
        assert!(themes.previous.is_none());
        assert_eq!(themes.shown().id, DEFAULT);
        assert!(layout.theme.is_none());
        assert!(layout.previous_theme.is_none());
    }

    #[test]
    fn resetting_an_option_to_default_does_not_pin_it() {
        let data = tempfile::tempdir().unwrap();
        let layout = Layout::default();
        let mut themes = Themes::read(data.path(), &layout);
        let id = themes
            .install_fetched(data.path(), &layout, &fetched_package_with_option())
            .unwrap();

        let non_default = themes
            .set_option(
                &id,
                "wide",
                serde_json::Value::Bool(true),
                Performance::Full,
                false,
            )
            .unwrap();
        assert_eq!(non_default["wide"], true);
        let reset = themes
            .set_option(
                &id,
                "wide",
                serde_json::Value::Bool(false),
                Performance::Full,
                false,
            )
            .unwrap();
        assert!(reset.is_empty());
        assert!(themes.package(&id).unwrap().choices.is_empty());
    }

    #[test]
    fn installed_trials_keep_revert_and_expire_without_persisting_early() {
        let data = tempfile::tempdir().unwrap();
        let layout = Layout {
            theme: Some("sakura".into()),
            ..Layout::default()
        };
        let mut themes = Themes::read(data.path(), &layout);
        let id = themes
            .install_fetched(data.path(), &layout, &fetched_package("1.0.0", "#2a1a3a"))
            .unwrap();

        themes.try_installed(&id).unwrap();
        assert_eq!(themes.chosen_id(), id);
        assert_eq!(themes.chosen.as_deref(), Some("sakura"));
        assert_eq!(themes.revert_trial(), Some(Some("sakura".into())));
        assert_eq!(themes.chosen_id(), "sakura");

        themes.try_installed(&id).unwrap();
        let expired_at = themes.trial.as_ref().unwrap().until;
        assert_eq!(themes.expire_trial(expired_at), Some(Some("sakura".into())));
        assert!(themes.trial().is_none());

        themes.try_installed(&id).unwrap();
        assert_eq!(themes.keep_trial(data.path(), &layout).unwrap(), Some(id));
        assert!(themes.trial().is_none());
        assert_eq!(themes.chosen.as_deref(), Some("sakura"));
    }

    #[test]
    fn auto_variant_follows_system_without_changing_the_persisted_id() {
        let data = tempfile::tempdir().unwrap();
        let layout = Layout::default();
        let mut themes = Themes::read(data.path(), &layout);
        let id = themes
            .install_fetched(data.path(), &layout, &fetched_dual_package())
            .unwrap();
        themes.choose(data.path(), &id);
        assert_eq!(themes.shown().chat, color(palette::YAP.chat));

        assert!(themes.system_mode_changed(Mode::Light));
        assert_eq!(
            themes.shown().chat,
            color(palette::find("sakura").unwrap().theme.chat)
        );
        assert_eq!(themes.chosen.as_deref(), Some(id.as_str()));

        themes
            .set_option(
                &id,
                options::MODE,
                serde_json::Value::String("dark".into()),
                Performance::Full,
                false,
            )
            .unwrap();
        assert_eq!(themes.shown().chat, color(palette::YAP.chat));
        assert!(!themes.system_mode_changed(Mode::Dark));
        assert!(!themes.system_mode_changed(Mode::Light));
        assert_eq!(themes.shown().chat, color(palette::YAP.chat));
    }

    #[test]
    fn reviewed_trial_installs_only_when_kept() {
        let data = tempfile::tempdir().unwrap();
        let layout = Layout {
            theme: Some("sakura".into()),
            ..Layout::default()
        };
        let fetched = fetched_package("1.0.0", "#2a1a3a");
        let mut themes = Themes::read(data.path(), &layout);

        themes.try_reviewed(&fetched, &layout).unwrap();
        assert_eq!(themes.chosen_id(), TRIAL_ID);
        assert!(themes.installed.is_empty());
        assert_eq!(themes.revert_trial(), Some(Some("sakura".into())));
        assert!(themes.installed.is_empty());

        themes.try_reviewed(&fetched, &layout).unwrap();
        let kept = themes.keep_trial(data.path(), &layout).unwrap().unwrap();
        assert_eq!(kept, "installed:com.example.plum");
        assert_eq!(themes.installed.len(), 1);
        assert_eq!(themes.chosen.as_deref(), Some("sakura"));
    }

    #[test]
    fn a_layout_without_a_theme_shows_bonfire_and_an_unknown_one_yap() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            Themes::read(folder.path(), &Layout::read(folder.path()))
                .shown()
                .id,
            "bonfire"
        );
        std::fs::write(
            folder.path().join(LAYOUT),
            r#"{"theme": "gone", "zoom": 1.2}"#,
        )
        .unwrap();
        assert_eq!(
            Themes::read(folder.path(), &Layout::read(folder.path()))
                .shown()
                .id,
            "yap"
        );
    }

    #[test]
    fn choosing_a_theme_keeps_the_rest_of_the_layout() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join(LAYOUT),
            r##"{"zoom": 1.2, "custom_themes": [{"id": "custom:1", "name": "Plum",
                "base": "sakura", "colours": {"chat": "#2a1a3a"}}]}"##,
        )
        .unwrap();
        let mut themes = Themes::read(folder.path(), &Layout::read(folder.path()));
        let plum = themes.find("custom:1").unwrap();
        assert!(!plum.dark, "Sakura's");
        assert_eq!(plum.chat, crate::style::hex(0x2a1a3a));
        themes.choose(folder.path(), "custom:1");
        crate::layout::flush(folder.path());
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.path().join(LAYOUT)).unwrap()).unwrap();
        assert_eq!(written["theme"], "custom:1");
        assert_eq!(written["zoom"], 1.2);
        assert_eq!(
            Themes::read(folder.path(), &Layout::read(folder.path())).shown(),
            plum
        );
    }

    #[test]
    fn the_worked_out_colours_follow_the_theme() {
        let yap = Themes {
            custom: Vec::new(),
            installed: Vec::new(),
            companions: Vec::new(),
            refused: Vec::new(),
            chosen: Some("yap".into()),
            previous: None,
            trial: None,
            creator_preview: None,
            system_mode: Mode::Dark,
            window: (1024, 768),
        }
        .shown();
        // Discord's menu on its dark theme is 40, 40, 45: a step over raised.
        let [r, g, b, _] = yap.menu.into_rgba8();
        assert!(r > 0x26 && g > 0x27 && b > 0x2e);
        assert_eq!(yap.danger_text, crate::style::hex(0xfa777c));
        let sakura = Themes {
            custom: Vec::new(),
            installed: Vec::new(),
            companions: Vec::new(),
            refused: Vec::new(),
            chosen: Some("sakura".into()),
            previous: None,
            trial: None,
            creator_preview: None,
            system_mode: Mode::Dark,
            window: (1024, 768),
        }
        .shown();
        assert_eq!(sakura.menu, sakura.raised);
        assert_eq!(sakura.danger_text, sakura.danger);
    }

    #[test]
    fn a_package_is_resolved_over_its_base_before_it_reaches_the_kit() {
        let package = yapfast_core::themes::Package::parse(
            r##"{
                "theme_api": 1,
                "id": "dev.example.plum",
                "name": "Plum",
                "version": "1.0.0",
                "variants": {
                    "dark": {
                        "base": "yap",
                        "colours": { "chat": "#2a1a3a", "accent": "#c04080" }
                    }
                }
            }"##,
        )
        .expect("a package");
        let resolved = package
            .resolve_with(
                package.mode_for(Mode::Dark),
                &yapfast_core::themes::options::Choices::new(),
            )
            .expect("resolved");
        let palette = resolved_palette(&resolved);
        let shown = theme("installed:dev.example.plum", &palette);
        assert_eq!(shown.chat, crate::style::hex(0x2a1a3a));
        assert_eq!(shown.accent, crate::style::hex(0xc04080));
        assert_eq!(palette.rail, palette::YAP.rail, "the base fills gaps");
    }

    #[test]
    fn resolved_shapes_spacing_and_motion_reach_the_kit_theme() {
        let package = yapfast_core::themes::Package::parse(
            r##"{
                "theme_api": 1,
                "id": "dev.example.metrics",
                "name": "Metrics",
                "version": "1.0.0",
                "colours": {
                    "chat": "linear(135deg, #102030, #405060)",
                    "members": "#203040",
                    "composer": "#304050"
                },
                "shape": {
                    "button": { "radius": 13, "border": 2, "border_colour": "#c04080" },
                    "avatar": { "radius": 20 },
                    "row": { "radius": 4, "inset": [8, 2], "stripe": { "colour": "#ffffff11", "lists": ["messages"] } },
                    "label": { "ground": "#224466", "text": "#ddeeff", "case": "upper" },
                    "card": { "inset_top": 12 },
                    "card_section": { "ground": "#182838", "bar": "#446688", "bar_text": "#ffffff", "case": "lower" },
                    "radii": { "small": 3, "medium": 7, "large": 11 },
                    "outline_width": 1.5,
                    "borders": { "dialog": { "style": "dotted", "gap": 3, "width": 2, "colour": "#141414" } }
                },
                "spacing": {
                    "message_gap": 5,
                    "channel_row_height": 39,
                    "sidebar_width": 333
                },
                "type": {
                    "body": { "size": 17, "weight": 300 },
                    "headings": { "size": 20, "weight": 800, "uppercase": true, "header_size": 22, "title_size": 28 },
                    "names": { "size": 16, "weight": 700 }
                },
                "layout": {
                    "channel_rows": { "height": 41 },
                    "member_rows": { "height": 52 }
                },
                "art": { "footer_height": 72, "surfaces": {
                    "chat": { "texture": "paper", "reserve": { "left": 24, "bottom": 16 } },
                    "footer": { "texture": "paper" }
                } },
                "motion": { "quick": 90, "popup": 240, "hover_in": 70, "hover_out": 110, "curve": "linear", "hover": "lift", "press": "scale", "open": "zoom" },
                "variants": { "dark": { "base": "yap" } }
            }"##,
        )
        .expect("a package");
        let resolved = package
            .resolve_with(
                package.mode_for(Mode::Dark),
                &yapfast_core::themes::options::Choices::new(),
            )
            .expect("resolved");
        let shown = theme(
            "installed:dev.example.metrics",
            &resolved_palette(&resolved),
        );
        let mut kit_metrics = metrics(&resolved, Performance::Full, false);
        kit_metrics.types = typography(Path::new("."), &resolved);
        shown.register_metrics(kit_metrics);
        assert!(matches!(
            shown.package_paint("chat"),
            Some(iced::Background::Gradient(_))
        ));
        assert_eq!(
            shown.package_paint("members"),
            Some(argb(0xff20_3040).into())
        );
        assert_eq!(
            shown.package_paint("composer"),
            Some(argb(0xff30_4050).into())
        );
        let border = shown.surface_border("button", 6.0, 1.0, Color::WHITE);
        assert_eq!(border.radius, 13.0.into());
        assert_eq!(border.width, 2.0);
        assert_eq!(border.color, argb(0xffc0_4080));
        assert_eq!(shown.surface_radius("field", 6.0), 7.0);
        assert_eq!(shown.avatar_radius(40.0, 20.0), 8.0);
        assert_eq!(shown.row_radius(0.0), 4.0);
        assert_eq!(shown.surface_inset("row"), Some((8.0, 2.0)));
        assert_eq!(shown.surface_inset_top("card"), Some(12.0));
        assert!(shown.has_surface("card_section"));
        assert_eq!(
            shown.surface_words("card_section", "Member Since"),
            "member since"
        );
        assert_eq!(
            shown.surface_bar("card_section"),
            Some((argb(0xff44_6688).into(), Some(Color::WHITE)))
        );
        assert_eq!(shown.surface_words("label", "People"), "PEOPLE");
        assert_eq!(shown.type_size("body", 14.0), 17.0);
        assert_eq!(shown.type_size("names", 14.0), 16.0);
        assert_eq!(shown.header_size(16.0), 22.0);
        assert_eq!(shown.title_size(24.0), 28.0);
        assert_eq!(shown.heading("Themes"), "THEMES");
        assert_eq!(
            shown.font("body", crate::fonts::BOLD).weight,
            iced::font::Weight::Light
        );
        assert_eq!(shown.surface_text("label"), Some(argb(0xffdd_eeff)));
        assert_eq!(
            shown.outline("dialog"),
            Some(yapfast_iced_kit::theme::Outline {
                stroke: yapfast_iced_kit::theme::OutlineStroke::Dotted,
                dash: 2.0,
                gap: 3.0,
                width: Some(2.0),
                colour: Some(argb(0xff14_1414)),
            })
        );
        assert!(
            shown
                .surface_stripe("row")
                .is_some_and(|stripe| stripe.messages)
        );
        assert_eq!(shown.spacing().message_gap, Some(5.0));
        assert_eq!(shown.spacing().channel_row_height, Some(41.0));
        assert_eq!(shown.spacing().member_row_height, Some(52.0));
        assert_eq!(shown.spacing().sidebar_width, Some(333.0));
        assert_eq!(shown.motion().popup, Duration::from_millis(240));
        assert_eq!(shown.motion().quick, Duration::from_millis(90));
        assert_eq!(shown.motion().hover_in, Duration::from_millis(70));
        assert_eq!(shown.motion().hover_out, Duration::from_millis(110));
        assert_eq!(shown.motion().hover, yapfast_iced_kit::theme::Hover::Lift);
        assert_eq!(shown.motion().press, yapfast_iced_kit::theme::Press::Scale);
        assert_eq!(
            shown.motion().open,
            yapfast_iced_kit::theme::Transition::Zoom
        );
        assert!(shown.decorated("chat"));
        assert!(shown.decorated("footer"));
        let art = theme_art(
            Path::new("."),
            &resolved,
            Performance::Balanced,
            false,
            (1024, 768),
        );
        assert_eq!(
            art.reserves.get(&ArtSurface::Chat),
            Some(&art::Reserve {
                left: 24.0,
                bottom: 16.0,
                ..art::Reserve::default()
            })
        );
        assert_eq!(art.footer_height, Some(72.0));

        shown.register_metrics(metrics(&resolved, Performance::Light, false));
        assert_eq!(shown.motion(), yapfast_iced_kit::theme::Motion::default());
    }

    #[test]
    fn turning_a_theme_off_keeps_none_and_shows_the_default() {
        let folder = tempfile::tempdir().unwrap();
        let mut themes = Themes::read(folder.path(), &Layout::default());
        themes.choose(folder.path(), "sakura");
        themes.turn_off(folder.path());
        layout::flush(folder.path());
        assert_eq!(themes.shown().id, DEFAULT);
        let layout = Layout::read(folder.path());
        assert!(layout.theme.is_none());
        assert_eq!(layout.previous_theme.as_deref(), Some("sakura"));
        assert_eq!(themes.previous(), Some(("sakura", "Sakura")));
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.path().join(LAYOUT)).unwrap()).unwrap();
        assert!(written["theme"].is_null());
        assert_eq!(written["previous_theme"], "sakura");
    }

    #[test]
    fn choosing_and_reverting_keep_the_owner_theme_history() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join(LAYOUT),
            r#"{"theme":"sakura","previous_theme":"yap","zoom":1.2}"#,
        )
        .unwrap();
        let mut themes = Themes::read(folder.path(), &Layout::read(folder.path()));
        assert_eq!(themes.previous(), Some(("yap", "Yap")));

        // Choosing what is already shown leaves Back useful.
        themes.choose(folder.path(), "sakura");
        assert_eq!(themes.previous(), Some(("yap", "Yap")));

        themes.choose(folder.path(), "bonfire");
        assert_eq!(themes.previous(), Some(("sakura", "Sakura")));
        assert!(themes.revert(folder.path()));
        assert_eq!(themes.chosen_id(), "sakura");
        assert_eq!(themes.previous(), Some(("bonfire", "Bonfire")));
        layout::flush(folder.path());

        let layout = Layout::read(folder.path());
        assert_eq!(layout.theme.as_deref(), Some("sakura"));
        assert_eq!(layout.previous_theme.as_deref(), Some("bonfire"));
        assert_eq!(layout.zoom, Some(1.2), "unrelated layout keys stay");
    }

    #[test]
    fn back_after_turning_off_restores_the_theme_once() {
        let folder = tempfile::tempdir().unwrap();
        let mut themes = Themes::read(folder.path(), &Layout::default());
        themes.choose(folder.path(), "sakura");
        themes.turn_off(folder.path());
        assert!(themes.revert(folder.path()));
        assert_eq!(themes.chosen_id(), "sakura");
        assert_eq!(themes.previous(), None);
        assert!(!themes.revert(folder.path()));
        layout::flush(folder.path());

        let layout = Layout::read(folder.path());
        assert_eq!(layout.theme.as_deref(), Some("sakura"));
        assert!(layout.previous_theme.is_none());
    }

    #[test]
    fn procedural_overlays_keep_their_scope_and_performance_cost() {
        let package = yapfast_core::themes::Package::parse(
            r##"{
                "theme_api": 1,
                "id": "dev.example.overlays",
                "name": "Overlays",
                "version": "1.0.0",
                "art": { "overlays": [
                    { "kind": "scanlines", "size": 4 },
                    { "kind": "flare", "on": "chat", "heavy": true }
                ] },
                "variants": { "dark": { "base": "yap" } }
            }"##,
        )
        .expect("an overlay package");
        let resolved = package
            .resolve_with(
                package.mode_for(Mode::Dark),
                &yapfast_core::themes::options::Choices::new(),
            )
            .expect("resolved");
        let balanced = theme_art(
            Path::new("."),
            &resolved,
            Performance::Balanced,
            false,
            (1024, 768),
        );
        assert_eq!(balanced.procedural_overlays.len(), 1);
        assert_eq!(balanced.chat_procedural_overlays.len(), 1);

        let light = theme_art(
            Path::new("."),
            &resolved,
            Performance::Light,
            false,
            (1024, 768),
        );
        assert_eq!(light.procedural_overlays.len(), 1);
        assert!(light.chat_procedural_overlays.is_empty());
    }

    #[test]
    fn pace_tooltip_cues_extra_colours_and_list_typography() {
        use yapfast_core::themes::lists::{FontRole, HoverGround};
        let _ = HoverGround::Hover;
        assert_eq!(
            pace_of(
                false,
                false,
                true,
                false,
                Performance::Full,
                false,
                package::Unfocused::Full
            ),
            Pace::Still
        );
        assert_eq!(
            pace_of(
                true,
                true,
                true,
                false,
                Performance::Full,
                false,
                package::Unfocused::Full
            ),
            Pace::Still
        );
        assert_eq!(
            pace_of(
                true,
                false,
                true,
                true,
                Performance::Full,
                false,
                package::Unfocused::Full
            ),
            Pace::Slow
        );
        assert_eq!(
            pace_of(
                true,
                false,
                false,
                false,
                Performance::Full,
                false,
                package::Unfocused::Slow
            ),
            Pace::Slow
        );
        assert_eq!(
            pace_of(
                true,
                false,
                false,
                false,
                Performance::Full,
                false,
                package::Unfocused::Full
            ),
            Pace::Full
        );
        assert_eq!(
            pace_of(
                true,
                false,
                false,
                false,
                Performance::Balanced,
                false,
                package::Unfocused::Full
            ),
            Pace::Still
        );
        assert_eq!(Pace::Slow.cap_fps(60), Pace::SLOW_FPS);
        assert_eq!(Pace::Still.cap_fps(60), 0);

        let package = yapfast_core::themes::Package::parse(
            r##"{
                "theme_api": 1,
                "id": "dev.example.parity",
                "name": "Parity",
                "version": "1.0.0",
                "based_on": "Bonfire",
                "colours": {
                    "accent-text": "#ffe082",
                    "on-tooltip": "#112233",
                    "composer-focus": "#445566",
                    "row-selected-text": "#778899",
                    "card-button": "#101010",
                    "mention-chip": "#223344",
                    "scrim": "#010203"
                },
                "art": { "surfaces": { "tooltip": { "texture": "paper", "reserve": { "left": 8, "right": 8 } } } },
                "motion": { "unfocused": "still" },
                "variants": { "dark": { "base": "yap" } }
            }"##,
        )
        .expect("package");
        let resolved = package
            .resolve_with(Mode::Dark, &yapfast_core::themes::options::Choices::new())
            .expect("resolved");
        let shown = theme("installed:dev.example.parity", &resolved_palette(&resolved));
        let mut kit_metrics = metrics(&resolved, Performance::Full, false);
        let art = theme_art(
            Path::new("."),
            &resolved,
            Performance::Full,
            false,
            (1024, 768),
        );
        kit_metrics.tooltip = tooltip_surface(&art);
        kit_metrics.hover_cue = true;
        shown.register_metrics(kit_metrics);

        assert_eq!(
            shown.motion().unfocused,
            yapfast_iced_kit::theme::Unfocused::Still
        );
        assert!(shown.tooltip_surface().is_some());
        assert_eq!(
            shown.tooltip_surface().and_then(|t| t.reserve),
            Some([0.0, 8.0, 0.0, 8.0])
        );
        assert!(shown.has_hover_cue());
        assert!(!shown.has_press_cue());
        assert_eq!(shown.accent_text(), argb(0xffff_e082));
        assert_eq!(
            shown.token_colour("on-tooltip", Color::BLACK),
            argb(0xff11_2233)
        );
        assert_eq!(
            shown.token_colour("composer-focus", Color::BLACK),
            argb(0xff44_5566)
        );
        assert_eq!(
            shown.token_colour("row-selected-text", Color::BLACK),
            argb(0xff77_8899)
        );
        assert_eq!(
            shown.token_colour("card-button", Color::BLACK),
            argb(0xff10_1010)
        );
        assert_eq!(
            shown.token_colour("mention-chip", Color::BLACK),
            argb(0xff22_3344)
        );
        assert_eq!(shown.token_colour("scrim", Color::BLACK), argb(0xff01_0203));

        let font = list_font(
            &shown,
            Some(FontRole::Headings),
            "body",
            Some(700.0),
            crate::fonts::REGULAR,
        );
        assert_eq!(font.weight, iced::font::Weight::Bold);
        assert!(
            offset_shadow(Some((0xffff_0000, 1.0, 2.0)))
                .shadow
                .is_some()
        );
    }
}
