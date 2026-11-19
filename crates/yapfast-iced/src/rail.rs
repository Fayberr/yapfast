//! The server rail down the window's left, with its Discord look: the DM
//! home (the app's logo), a line, the DMs with unread messages, then each
//! server and folder, and the button to join a server at the foot. Each
//! tile has its pill at the rail's edge (tall while open, half under the
//! pointer, a dot while unread), its mentions in a red badge, a speaker
//! while this account is in a call there, and its name in a tooltip to the
//! right.

use crate::widgets::scroll_view;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};

use iced::{
    Alignment, Color, Element, Length, Padding, Size, Task,
    widget::{
        Space, column, container, mouse_area, row, scrollable, scrollable::Viewport, stack, text,
        tooltip,
    },
};
use yapfast_core::{
    application::{Action, AppState, Navigation},
    chat::{ChatRoom, ChatServer},
    discord,
    themes::lists::{LabelPlace, PillMode, RailShape, RailTiles},
};

use crate::{
    app::{App, Message, key},
    controls::{self, RAIL_COUNT, bar, count_badge, count_pill},
    fonts::{BOLD, MEDIUM},
    pictures::{Icon, initials},
    style::{corners, hex, rounded, scrollbar},
    sync::MARGIN,
    sync::Scroll,
    widgets::{Pic, RightPress, Rows},
};

/// The server rail: its DM home, its rows and its viewport.
pub struct Rail {
    pub home: Home,
    pub items: Vec<RailItem>,
    pub scroll: crate::sync::Scroll,
    /// A tree layout's selected server or DM branch is folded. This is
    /// window state, like the old Slint tree's fold, not a preference.
    pub tree_folded: bool,
}

/// The rail, down the window's left edge.
pub fn view<'a>(rail: &'a Rail, cx: crate::ctx::Ctx<'a>) -> Element<'a, Message> {
    View { s: rail, cx }.rail_view()
}

/// The same server picker laid across the window's top. The item faces and
/// owner-theme tile art stay on the same path as the vertical rail; only the
/// scroll axis and edge marks change.
pub fn top_view<'a>(rail: &'a Rail, cx: crate::ctx::Ctx<'a>) -> Element<'a, Message> {
    View { s: rail, cx }.top_view()
}

/// A theme's file-explorer arrangement: servers and folders become text
/// rows in the sidebar, immediately above the selected server's channels.
pub fn tree_head_view<'a>(rail: &'a Rail, cx: crate::ctx::Ctx<'a>) -> Element<'a, Message> {
    View { s: rail, cx }.tree_view(true)
}

/// The server rows after the selected server's channel branch.
pub fn tree_tail_view<'a>(rail: &'a Rail, cx: crate::ctx::Ctx<'a>) -> Element<'a, Message> {
    View { s: rail, cx }.tree_view(false)
}

/// The rail's view: its rows and the view context.
#[derive(Clone, Copy)]
struct View<'a> {
    s: &'a Rail,
    cx: crate::ctx::Ctx<'a>,
}

impl Rail {
    pub fn new(height: f32) -> Self {
        Self {
            home: Home::default(),
            items: Vec::new(),
            scroll: crate::sync::Scroll::new(height),
            tree_folded: false,
        }
    }
}

pub const WIDTH: f32 = 72.0;

/// The rail's resolved width. An installed package may need a little more
/// room for card, ticket or tall tiles; stock and colour-only themes keep
/// Wired's exact 72 px rail.
pub fn themed_width(package: Option<&yapfast_core::themes::layout::Layout>) -> f32 {
    package
        .and_then(|layout| layout.rail_tiles.as_ref())
        .map_or(WIDTH, RailTiles::rail_width)
}

/// The rail width after ordinary chrome is considered. A package's text
/// rail owns its checked width; icon and tile rails keep the layout width.
pub fn resolved_width(
    package: Option<&yapfast_core::themes::layout::Layout>,
    chrome: Option<&yapfast_core::themes::chrome::Chrome>,
) -> f32 {
    chrome
        .filter(|chrome| chrome.rail_text())
        .and_then(|chrome| chrome.rail_width)
        .unwrap_or_else(|| themed_width(package))
}

/// A top rail is 64 px in stock themes. Owner tiles may make it taller, but
/// never shorter than Wired's usable hit area.
pub fn themed_top_height(package: Option<&yapfast_core::themes::layout::Layout>) -> f32 {
    package
        .and_then(|layout| layout.rail_tiles.as_ref())
        .map_or(64.0, |tiles| {
            let label = if tiles.label == LabelPlace::Below {
                tiles.label_size + 2.0
            } else {
                0.0
            };
            (tiles.height + label + 16.0).max(64.0)
        })
}
/// A server's tile, a folder's, and the gap after each, as Discord's.
pub const TILE: f32 = 40.0;
pub const FOLDER: f32 = 48.0;
pub const GAP: f32 = 8.0;
/// A tile's corners: a server's, and a folder's.
const RADIUS: f32 = 12.0;
const FOLDER_RADIUS: f32 = 16.0;
/// Room left above each tile for the call speaker, which stands 3 px above
/// it, and below it for the badge, 4 px past it.
const ABOVE: f32 = 3.0;
const BELOW: f32 = 4.0;
/// Discord's colour for a folder that wasn't given one.
const FOLDER_COLOR: u32 = 0x5865f2;
/// The pill at the rail's edge: open, pointed at, unread.
const PILL_OPEN: f32 = 40.0;
const PILL_POINTED: f32 = 20.0;
const PILL_UNREAD: f32 = 8.0;

/// What's done on the rail.
#[derive(Clone, Debug)]
pub enum Event {
    Scrolled(Viewport),
    /// The pointer came onto a tile or left it, by its id ("" for the home,
    /// "+" for the join button).
    Point(String, bool),
    Home,
    Server(String),
    Folder(String),
    /// An unread DM on the rail opens.
    Dm(String),
    Join,
    /// A right click on a server, a folder or an unread DM: its menu, where
    /// the pointer was pressed.
    Menu(String, iced::Point),
}

/// The DM home's marks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Home {
    pub selected: bool,
    pub unread: bool,
    /// DM mentions and incoming friend requests.
    pub badge: String,
}

/// What a row of the rail is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Server,
    /// An unread DM, above the servers.
    Dm,
    /// The line between the unread DMs and the servers.
    Divider,
    /// A folder, open or closed (with its first four servers' initials and
    /// icons), in its colour.
    Folder {
        open: bool,
        tint: u32,
        preview: Vec<(String, Option<String>)>,
    },
    /// A server in an open folder, and whether it's the folder's last.
    Nested {
        tint: u32,
        last: bool,
    },
}

/// One row of the rail.
#[derive(Clone, Debug, PartialEq)]
pub struct RailItem {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub kind: Kind,
    pub selected: bool,
    pub unread: bool,
    /// Mentions, as the badge shows them ("3", "99+"); empty for none.
    pub badge: String,
    /// This account is in a call in one of its voice channels.
    pub call: bool,
    /// Out of reach in a Discord outage, shown faded.
    pub unavailable: bool,
}

impl RailItem {
    fn server(server: &ChatServer, selected: bool, (unread, mentions): (bool, u64)) -> Self {
        Self {
            id: server.id.clone(),
            name: server.name.clone(),
            icon: server.icon.clone(),
            kind: Kind::Server,
            selected,
            unread,
            badge: count_badge(mentions),
            call: false,
            unavailable: server.unavailable,
        }
    }

    /// The row's height, its gap included.
    pub fn height(&self, index: usize, tiles: Option<&RailTiles>) -> f32 {
        let tile = tiles.map_or(TILE, |tiles| tiles.height);
        let folder = tiles.map_or(FOLDER, |tiles| tiles.height);
        let gap = tiles.map_or(GAP, |tiles| tiles.gap + 2.0 * tiles.pad);
        let label = tiles.map_or(0.0, |tiles| {
            if tiles.label == LabelPlace::Below {
                tiles.label_size + 2.0
            } else {
                0.0
            }
        });
        match &self.kind {
            Kind::Divider if index == 0 => 0.0,
            Kind::Divider => {
                tiles
                    .and_then(|tiles| tiles.divider)
                    .map_or(1.0, |divider| divider.size.1)
                    + gap
            }
            Kind::Folder { open: true, .. } => folder + label + 4.0,
            Kind::Folder { open: false, .. } => folder + label + gap,
            Kind::Nested { last, .. } => tile + label + if *last { 4.0 } else { 0.0 } + gap,
            Kind::Server | Kind::Dm => tile + label + gap,
        }
    }
}

/// The DM home's marks, from the core's state: selected, any DM unread, and
/// the DM mentions with the friend requests waiting, as Discord counts them.
pub fn home(state: &AppState) -> Home {
    let mentions: u64 = state
        .rooms
        .iter()
        .filter(|room| room.direct)
        .map(|room| room.mentions)
        .sum();
    let incoming = state
        .friend_requests
        .iter()
        .filter(|request| request.incoming && !request.spam)
        .count() as u64;
    Home {
        selected: state.navigation == Navigation::DirectMessages,
        unread: state
            .rooms
            .iter()
            .any(|room| room.direct && room.unread > 0),
        badge: count_badge(mentions + incoming),
    }
}

/// The rail's rows, from the core's state: the unread DMs (in the core's
/// order, latest first), a line, then each server, and each folder with its
/// servers after it while it's open.
pub fn rail(state: &AppState, open_folders: &BTreeSet<String>) -> Vec<RailItem> {
    let rooms: HashMap<&str, &ChatRoom> = state
        .rooms
        .iter()
        .map(|room| (room.id.as_str(), &**room))
        .collect();
    let mut items: Vec<RailItem> = state
        .rooms
        .iter()
        .filter(|room| {
            room.direct
                && !room.request
                && room.category != discord::SPAM
                && (room.unread > 0 || room.mentions > 0)
        })
        .map(|room| RailItem {
            id: room.id.clone(),
            name: room.name.clone(),
            icon: room.avatar.clone(),
            kind: Kind::Dm,
            selected: false,
            unread: true,
            // Discord counts each of a DM's messages; Matrix counts its
            // unread messages apart from its mentions.
            badge: count_badge(room.mentions.max(room.unread)),
            call: false,
            unavailable: false,
        })
        .collect();
    items.push(RailItem {
        id: "divider".into(),
        name: String::new(),
        icon: None,
        kind: Kind::Divider,
        selected: false,
        unread: false,
        badge: String::new(),
        call: false,
        unavailable: false,
    });
    // A server's channels: whether any is unread, and its mentions. One
    // its channel list leaves out, or a Discord voice channel's chat, marks
    // it only by mentions, as in Discord's client (GuildReadStateStore).
    let marks = |server: &ChatServer| {
        let channels = || {
            server
                .channels
                .iter()
                .filter_map(|channel| rooms.get(channel.as_str()))
                .filter(|room| !room.direct)
        };
        (
            channels().any(|room| {
                room.mentions > 0
                    || (room.unread > 0
                        && !server.muted
                        && !room.unlisted
                        && !(server.discord && room.voice))
            }),
            channels().map(|room| room.mentions).sum::<u64>(),
        )
    };
    let selected = |server: &ChatServer| state.navigation == Navigation::Server(server.id.clone());
    let call = state
        .voice
        .active()
        .then_some(state.voice.room.as_deref())
        .flatten();
    let in_call = |server: &ChatServer| {
        call.is_some_and(|room| server.channels.iter().any(|channel| channel == room))
    };
    let mut servers = state.servers.iter().peekable();
    while let Some(server) = servers.next() {
        let Some(folder) = &server.folder else {
            items.push(RailItem {
                call: in_call(server),
                ..RailItem::server(server, selected(server), marks(server))
            });
            continue;
        };
        let mut inside = vec![server];
        while let Some(next) = servers.next_if(|next| next.folder.as_ref() == Some(folder)) {
            inside.push(next);
        }
        let open = open_folders.contains(&folder.id);
        let tint = folder.color.unwrap_or(FOLDER_COLOR);
        let (unread, mentions) = inside
            .iter()
            .map(|server| marks(server))
            .fold((false, 0), |(unread, mentions), (one, more)| {
                (unread || one, mentions + more)
            });
        let name = folder.name.clone().unwrap_or_else(|| {
            let mut names: Vec<_> = inside
                .iter()
                .take(3)
                .map(|server| server.name.as_str())
                .collect();
            if inside.len() > 3 {
                names.push("…");
            }
            names.join(", ")
        });
        items.push(RailItem {
            id: folder.id.clone(),
            name,
            icon: None,
            kind: Kind::Folder {
                open,
                tint,
                preview: inside
                    .iter()
                    .take(4)
                    .map(|server| (server_initials(&server.name), server.icon.clone()))
                    .collect(),
            },
            selected: inside.iter().any(|server| selected(server)),
            unread,
            badge: count_badge(mentions),
            call: inside.iter().any(|server| in_call(server)),
            unavailable: false,
        });
        if open {
            let last = inside.len() - 1;
            for (index, server) in inside.into_iter().enumerate() {
                items.push(RailItem {
                    kind: Kind::Nested {
                        tint,
                        last: index == last,
                    },
                    call: in_call(server),
                    ..RailItem::server(server, selected(server), marks(server))
                });
            }
        }
    }
    items
}

/// A server's letters without its icon: the first letter of up to three
/// words, as "Co-op crew" makes "CC".
pub fn server_initials(name: &str) -> String {
    let shown: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(3)
        .flat_map(char::to_uppercase)
        .collect();
    if shown.is_empty() { "?".into() } else { shown }
}

/// A tile's letters: a server's initials, a DM's first letter.
fn tile_initials(item: &RailItem) -> String {
    match item.kind {
        Kind::Dm => initials(&item.name),
        _ => server_initials(&item.name),
    }
}

/// Where a tree inserts the selected server's channels: after its own row,
/// or after the closed folder that contains it. Direct messages have no
/// server pivot, so their rows sit between the tree head and every server.
fn tree_at(items: &[RailItem]) -> Option<usize> {
    items.iter().position(|item| {
        item.selected
            && item.kind != Kind::Divider
            && item.kind != Kind::Dm
            && !matches!(item.kind, Kind::Folder { open: true, .. })
    })
}

/// What a tile shows around it.
#[derive(Default)]
struct Marks<'a> {
    selected: bool,
    unread: bool,
    badge: &'a str,
    call: bool,
    pointed: bool,
    folder: bool,
    open_folder: bool,
    /// A right click on it opens its menu.
    menu: bool,
}

/// What's done on the rail.
pub fn update(app: &mut App, event: Event) -> Task<Message> {
    match event {
        Event::Scrolled(viewport) => {
            app.rail.scroll = Scroll::of(viewport);
            return app.ask();
        }
        Event::Point(id, entered) => {
            crate::app::point(&mut app.pointed.rail, id, entered);
            return Task::none();
        }
        Event::Home => {
            if tree_mode(app) && app.client.state().navigation == Navigation::DirectMessages {
                app.rail.tree_folded = !app.rail.tree_folded;
                return app.ask();
            }
            app.rail.tree_folded = false;
            app.client
                .dispatch(Action::Navigate(Navigation::DirectMessages));
        }
        Event::Server(id) => {
            if tree_mode(app) && app.client.state().navigation == Navigation::Server(id.clone()) {
                app.rail.tree_folded = !app.rail.tree_folded;
                return app.ask();
            }
            app.rail.tree_folded = false;
            app.client
                .dispatch(Action::Navigate(Navigation::Server(id)));
        }
        Event::Folder(id) => {
            app.lists_shown = None;
            let was_open = app.layout.open_folders.remove(&id);
            let one_at_a_time = app
                .themes
                .package_layout(&app.theme.id)
                .is_some_and(|layout| {
                    layout.rail == Some(yapfast_core::themes::layout::RailSide::Tree)
                        && !layout.tree.expand_many
                });
            if !was_open {
                if one_at_a_time {
                    app.layout.open_folders.clear();
                }
                app.layout.open_folders.insert(id);
            }
            crate::layout::store(app.client.root(), "open_folders", &app.layout.open_folders);
            app.rail.items = rail(app.client.state(), &app.layout.open_folders);
            return app.ask();
        }
        Event::Dm(id) => {
            app.rail.tree_folded = false;
            app.client.dispatch(Action::Open(id));
        }
        Event::Join => app.client.dispatch(Action::OpenJoinServer),
        Event::Menu(id, at) => {
            return crate::place_menus::open_place_menu(app, &id, at, false);
        }
    }
    app.sync()
}

fn tree_mode(app: &App) -> bool {
    app.themes
        .package_layout(&app.theme.id)
        .is_some_and(|layout| layout.rail == Some(yapfast_core::themes::layout::RailSide::Tree))
}

impl<'a> View<'a> {
    fn tiles(self) -> Option<&'a RailTiles> {
        self.cx.themes.rail_tiles(&self.cx.look.theme.id)
    }

    fn rail_width(self) -> f32 {
        resolved_width(
            self.cx.themes.package_layout(&self.cx.look.theme.id),
            self.cx.themes.chrome(&self.cx.look.theme.id),
        )
    }

    fn top_rail(self) -> bool {
        self.cx
            .themes
            .package_layout(&self.cx.look.theme.id)
            .and_then(|layout| layout.rail)
            == Some(yapfast_core::themes::layout::RailSide::Top)
    }

    fn tile_size(self) -> (f32, f32) {
        self.tiles()
            .map_or((TILE, TILE), |tiles| (tiles.width, tiles.height))
    }

    fn folder_size(self) -> (f32, f32) {
        self.tiles()
            .map_or((FOLDER, FOLDER), |tiles| (tiles.width, tiles.height))
    }

    fn label_height(self) -> f32 {
        self.tiles().map_or(0.0, |tiles| {
            if tiles.label == LabelPlace::Below {
                tiles.label_size + 2.0
            } else {
                0.0
            }
        })
    }

    fn tile_radius(self) -> f32 {
        self.tiles().map_or_else(
            || self.cx.look.theme.surface_radius("rail-icon", RADIUS),
            |tiles| {
                if tiles.shape == RailShape::Circle {
                    tiles.width.min(tiles.height) / 2.0
                } else {
                    tiles.radius
                }
            },
        )
    }

    pub fn rail_view(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        if self
            .cx
            .themes
            .chrome(&theme.id)
            .is_some_and(yapfast_core::themes::chrome::Chrome::rail_text)
        {
            return self.text_rail_view();
        }
        let tiles = self.tiles();
        let rail_width = self.rail_width();
        let viewport = self.s.scroll;
        let mut top = 0.0;
        let mut rows = Rows::new(
            self.s
                .items
                .iter()
                .enumerate()
                .map(|(i, item)| item.height(i, tiles))
                .sum(),
        );
        for (index, item) in self.s.items.iter().enumerate() {
            let height = item.height(index, tiles);
            let at = top;
            top += height;
            if at + height < viewport.offset - MARGIN
                || at > viewport.offset + viewport.height + MARGIN
            {
                continue;
            }
            // Each row reaches above its tile for the call speaker.
            rows.push(
                key(&item.id),
                at - ABOVE,
                height + ABOVE,
                self.rail_row(index, item, height),
            );
        }
        let list = scroll_view(rows)
            .on_scroll(|viewport| Message::Rail(Event::Scrolled(viewport)))
            .height(Length::Fill)
            .style(scrollbar(theme));
        let line = container(bar(theme.outline, 32, 1)).center_x(rail_width);
        let mut items = column![self.home_tile(), line, list]
            .spacing(tiles.map_or(GAP, |tiles| tiles.gap))
            .width(rail_width);
        let state = self.cx.state;
        if state.discord_online || state.preview {
            if let Some(gap) = tiles.and_then(|tiles| tiles.add_gap) {
                items = items.push(Space::new().height(gap));
            }
            items = items.push(self.action_tile(
                "+",
                Icon::Plus,
                "Join a server",
                Message::Rail(Event::Join),
            ));
        }
        let content: Element<'a, Message> = container(items.padding(Padding {
            bottom: 12.0,
            ..Padding::ZERO
        }))
        .width(rail_width)
        .height(Length::Fill)
        .into();
        let mut layers = stack![];
        if let Some(tiles) = tiles {
            for ground in &tiles.ground {
                if let Some(art) = crate::theme_art::tile_art(
                    self.cx,
                    yapfast_core::themes::package::art::ArtSurface::Rail,
                    ground,
                    rail_width,
                    self.cx.look.window.height,
                ) {
                    layers = layers.push(art);
                }
            }
        }
        layers
            .push(content)
            .width(rail_width)
            .height(Length::Fill)
            .into()
    }

    /// A theme's text rail: the same navigation and menus as the tile rail,
    /// with only checked labels and marks replacing its visible words.
    fn text_rail_view(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let chrome = self
            .cx
            .themes
            .chrome(&theme.id)
            .expect("a text rail belongs to installed chrome");
        let width = self.rail_width();
        let role = chrome
            .font
            .map_or("small", yapfast_core::themes::package::Role::name);
        let fixed = |words: String| {
            text(words)
                .size(theme.type_size(role, 14.0))
                .font(theme.font(role, MEDIUM))
                .color(theme.muted)
        };
        let mut content = column![].spacing(1).padding(8).width(Length::Fill);
        if let Some(title) = chrome
            .labels
            .get(&yapfast_core::themes::chrome::Label::RailTitle)
        {
            content = content
                .push(container(fixed(title.clone())).padding(Padding::ZERO.left(8).bottom(4)));
        }
        let home = self.cx.themes.label(
            &theme.id,
            yapfast_core::themes::chrome::Label::RailHome,
            "Direct messages",
            &[],
        );
        content = content.push(self.tree_button(
            String::new(),
            home,
            self.s.home.selected,
            self.s.home.unread,
            &self.s.home.badge,
            false,
            Message::Rail(Event::Home),
        ));

        let mut rows = column![].spacing(1).width(Length::Fill);
        for item in &self.s.items {
            if item.kind == Kind::Divider {
                rows = rows.push(Space::new().height(6));
                continue;
            }
            let (label, nested, selected, unread, badge, message) = match &item.kind {
                Kind::Folder { open, .. } => {
                    let label = chrome.folders.as_ref().map_or_else(
                        || format!("{} {}", if *open { "-" } else { "+" }, item.name),
                        |folders| {
                            format!(
                                "{}{}{}",
                                if *open {
                                    &folders.open
                                } else {
                                    &folders.closed
                                },
                                item.name,
                                folders.suffix
                            )
                        },
                    );
                    (
                        label,
                        false,
                        item.selected && !*open,
                        item.unread && !*open,
                        if *open { "" } else { item.badge.as_str() },
                        Message::Rail(Event::Folder(item.id.clone())),
                    )
                }
                Kind::Dm => (
                    format!("@{}", item.name),
                    false,
                    false,
                    true,
                    item.badge.as_str(),
                    Message::Rail(Event::Dm(item.id.clone())),
                ),
                Kind::Nested { .. } => (
                    self.cx.themes.marked(
                        &theme.id,
                        yapfast_core::themes::chrome::Place::Server,
                        &item.name,
                    ),
                    true,
                    item.selected,
                    item.unread,
                    item.badge.as_str(),
                    Message::Rail(Event::Server(item.id.clone())),
                ),
                Kind::Server => (
                    self.cx.themes.marked(
                        &theme.id,
                        yapfast_core::themes::chrome::Place::Server,
                        &item.name,
                    ),
                    false,
                    item.selected,
                    item.unread,
                    item.badge.as_str(),
                    Message::Rail(Event::Server(item.id.clone())),
                ),
                Kind::Divider => unreachable!(),
            };
            rows = rows.push(self.tree_button(
                item.id.clone(),
                label,
                selected,
                unread,
                badge,
                nested,
                message,
            ));
        }
        content = content.push(
            scroll_view(rows)
                .height(Length::Fill)
                .style(scrollbar(theme)),
        );
        if self.cx.state.discord_online || self.cx.state.preview {
            content = content.push(self.tree_button(
                "+".into(),
                "+ join".into(),
                false,
                false,
                "",
                false,
                Message::Rail(Event::Join),
            ));
        }
        let selected =
            self.s.items.iter().find(|item| {
                item.selected && matches!(item.kind, Kind::Server | Kind::Nested { .. })
            });
        if let (Some(template), Some(server)) = (
            chrome
                .labels
                .get(&yapfast_core::themes::chrome::Label::RailFooter),
            selected,
        ) {
            let words =
                yapfast_core::themes::chrome::fill(template, &[("server", server.name.as_str())]);
            content = content.push(container(fixed(words)).padding(Padding::ZERO.left(8).top(4)));
        }
        container(content).width(width).height(Length::Fill).into()
    }

    pub fn top_view(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let tiles = self.tiles();
        let height = themed_top_height(self.cx.themes.package_layout(&theme.id));
        let cell_width = self.tile_size().0.max(self.folder_size().0).max(40.0) + 20.0;
        let mut cells = row![container(self.home_tile()).width(cell_width).height(height)]
            .align_y(Alignment::Center)
            .spacing(0);
        for (index, item) in self.s.items.iter().enumerate() {
            if item.kind == Kind::Divider {
                if index > 0 {
                    let (width, colour, dotted) = tiles.and_then(|tiles| tiles.divider).map_or(
                        (1.0, theme.outline, false),
                        |divider| {
                            (
                                divider.size.1.max(1.0),
                                crate::themes::argb(divider.colour),
                                divider.dotted,
                            )
                        },
                    );
                    let rule: Element<'a, Message> = if dotted {
                        let mut dots = column![].spacing(width);
                        let count = ((height - 24.0) / (2.0 * width)).floor().max(1.0) as usize;
                        for _ in 0..count {
                            dots = dots.push(
                                container(Space::new())
                                    .width(width)
                                    .height(width)
                                    .style(move |_| rounded(colour, width / 2.0)),
                            );
                        }
                        dots.into()
                    } else {
                        bar(colour, width, 32).into()
                    };
                    cells = cells.push(container(rule).center_x(9).center_y(height));
                }
                continue;
            }
            cells = cells.push(
                container(self.rail_row(index, item, height))
                    .width(cell_width)
                    .height(height),
            );
        }
        let state = self.cx.state;
        if state.discord_online || state.preview {
            if let Some(gap) = tiles.and_then(|tiles| tiles.add_gap) {
                cells = cells.push(Space::new().width(gap));
            }
            cells = cells.push(
                container(self.action_tile(
                    "+",
                    Icon::Plus,
                    "Join a server",
                    Message::Rail(Event::Join),
                ))
                .width(cell_width)
                .height(height),
            );
        }
        let content = scrollable(container(cells).padding([0.0, 6.0]))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            ))
            .width(Length::Fill)
            .height(height)
            .style(scrollbar(theme));
        let mut layers = stack![];
        if let Some(tiles) = tiles {
            for ground in &tiles.ground {
                if let Some(art) = crate::theme_art::tile_art(
                    self.cx,
                    yapfast_core::themes::package::art::ArtSurface::Rail,
                    ground,
                    self.cx.look.window.width,
                    height,
                ) {
                    layers = layers.push(art);
                }
            }
        }
        layers
            .push(content)
            .width(Length::Fill)
            .height(height)
            .into()
    }

    pub fn tree_view(self, head: bool) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let at = tree_at(&self.s.items);
        let shown = self
            .s
            .items
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                item.kind != Kind::Divider
                    && if head {
                        at.is_some_and(|at| *index <= at)
                    } else {
                        at.is_none_or(|at| *index > at)
                    }
            })
            .count();
        let fixed = if head { 2 } else { 0 };
        let cap = if head { 210.0 } else { 150.0 };
        let height = ((shown + fixed) as f32 * 27.0 + 8.0).min(cap);
        let tree = self
            .cx
            .themes
            .package_layout(&theme.id)
            .map(|layout| &layout.tree);
        let stock_root = tree.map_or("~/", |tree| tree.root.as_str());
        let selected_server = self
            .s
            .items
            .iter()
            .find(|item| item.selected && matches!(item.kind, Kind::Server | Kind::Nested { .. }))
            .map_or("", |item| item.name.as_str());
        let root = self.cx.themes.label(
            &theme.id,
            yapfast_core::themes::chrome::Label::TreeRoot,
            stock_root,
            &[("server", selected_server)],
        );
        let stock_dms = tree.map_or("dm", |tree| tree.dms.as_str());
        let dms = self.cx.themes.label(
            &theme.id,
            yapfast_core::themes::chrome::Label::RailHome,
            stock_dms,
            &[],
        );
        let mut rows = column![].spacing(1).width(Length::Fill);
        if head {
            rows = rows
                .push(
                    container(text(root).size(13).font(MEDIUM).color(theme.muted))
                        .padding([6.0, 10.0]),
                )
                .push(self.tree_home(&dms));
        }
        for (index, item) in self.s.items.iter().enumerate() {
            let in_part = if head {
                at.is_some_and(|at| index <= at)
            } else {
                at.is_none_or(|at| index > at)
            };
            if in_part && item.kind != Kind::Divider {
                rows = rows.push(self.tree_row(item));
            }
        }
        scrollable(rows.padding([0.0, 6.0]))
            .height(height)
            .style(scrollbar(theme))
            .into()
    }

    fn tree_home(self, label: &str) -> Element<'a, Message> {
        self.tree_button(
            "".into(),
            label.to_owned(),
            self.s.home.selected,
            self.s.home.unread,
            &self.s.home.badge,
            false,
            Message::Rail(Event::Home),
        )
    }

    fn tree_row(self, item: &'a RailItem) -> Element<'a, Message> {
        if item.kind == Kind::Divider {
            return container(bar(self.cx.look.theme.outline, Length::Fill, 1))
                .padding([4.0, 10.0])
                .into();
        }
        let (label, nested, message) = match &item.kind {
            Kind::Folder { open, .. } => (
                format!("{} {}", if *open { "▾" } else { "▸" }, item.name),
                false,
                Message::Rail(Event::Folder(item.id.clone())),
            ),
            Kind::Dm => (
                format!("@{}", item.name),
                false,
                Message::Rail(Event::Dm(item.id.clone())),
            ),
            Kind::Nested { .. } => (
                format!(
                    "  {}",
                    self.cx.themes.marked(
                        &self.cx.look.theme.id,
                        yapfast_core::themes::chrome::Place::Server,
                        &item.name,
                    )
                ),
                true,
                Message::Rail(Event::Server(item.id.clone())),
            ),
            Kind::Server => (
                self.cx.themes.marked(
                    &self.cx.look.theme.id,
                    yapfast_core::themes::chrome::Place::Server,
                    &item.name,
                ),
                false,
                Message::Rail(Event::Server(item.id.clone())),
            ),
            Kind::Divider => unreachable!(),
        };
        let open_folder = matches!(item.kind, Kind::Folder { open: true, .. });
        self.tree_button(
            item.id.clone(),
            label,
            item.selected && !open_folder,
            item.unread && !open_folder,
            if open_folder { "" } else { &item.badge },
            nested,
            message,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn tree_button(
        self,
        id: String,
        label: String,
        selected: bool,
        unread: bool,
        badge: &str,
        nested: bool,
        message: Message,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let role = self
            .cx
            .themes
            .chrome(&theme.id)
            .and_then(|chrome| chrome.font)
            .map_or("body", yapfast_core::themes::package::Role::name);
        let pointed = self.cx.pointed.rail.as_deref() == Some(id.as_str());
        let ink = if selected || unread {
            theme.text
        } else {
            theme.muted
        };
        let mark = if selected {
            "●"
        } else if unread {
            "•"
        } else {
            " "
        };
        let mut line = row![
            text(mark)
                .size(11)
                .color(if selected { theme.accent } else { ink }),
            text(label)
                .size(theme.type_size(role, 14.0))
                .font(theme.font(role, MEDIUM))
                .color(ink)
                .width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .spacing(5);
        if !badge.is_empty() {
            line = line.push(count_pill(theme, badge, RAIL_COUNT, Some(theme.sidebar)));
        }
        let ground = if selected {
            theme.active
        } else if pointed {
            theme.hover
        } else {
            Color::TRANSPARENT
        };
        let row = container(line)
            .height(26)
            .width(Length::Fill)
            .padding(Padding {
                left: if nested { 20.0 } else { 8.0 },
                right: 7.0,
                top: 0.0,
                bottom: 0.0,
            })
            .style(move |_| rounded(ground, theme.surface_radius("row", 4.0)));
        let hit = mouse_area(row)
            .interaction(iced::mouse::Interaction::Pointer)
            .on_enter(Message::Rail(Event::Point(id.clone(), true)))
            .on_exit(Message::Rail(Event::Point(id.clone(), false)))
            .on_press(message);
        if id.is_empty() || id == "+" {
            hit.into()
        } else {
            let spot = self.cx.pressed;
            RightPress::new(hit, move |_| {
                Message::Rail(Event::Menu(id.clone(), spot.get()))
            })
            .into()
        }
    }

    /// The DM home: the app's logo, as Discord puts its own there.
    fn home_tile(self) -> Element<'a, Message> {
        let home = &self.s.home;
        let (width, height) = self.tile_size();
        let pointed = self.cx.pointed.rail.as_deref() == Some("");
        let face = self.glyph_tile(Icon::Logo, home.selected || pointed);
        let face = crate::theme_effects::rail_icon(face, self.cx, true, 0);
        let marks = Marks {
            selected: home.selected,
            unread: home.unread,
            badge: &home.badge,
            pointed,
            ..Marks::default()
        };
        // Its badge reaches into the gap below it.
        container(self.tile_cell(
            String::new(),
            "Direct messages".into(),
            face,
            (width, height, 0.0),
            marks,
            Message::Rail(Event::Home),
        ))
        .height(height + self.label_height())
        .into()
    }

    /// A tile with one of the icons on the raised ground, in the accent;
    /// lit, on the accent.
    fn glyph_tile(self, icon: Icon, lit: bool) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let (width, height) = self.tile_size();
        let radius = self.tile_radius();
        let mut border = theme.surface_border("rail-icon", RADIUS, 0.0, Color::TRANSPARENT);
        border.radius = radius.into();
        let shadow = theme.surface_shadow("rail-icon", iced::Shadow::default());
        let (ground, ink) = if lit {
            (theme.accent, theme.on_accent)
        } else {
            (theme.raised, theme.accent)
        };
        self.cx
            .look
            .glyph(icon, 24, ink)
            .boxed(Length::Fill)
            .width(width)
            .height(height)
            .style(move |_| container::Style {
                background: Some(ground.into()),
                border,
                shadow,
                ..Default::default()
            })
            .into()
    }

    /// A tile at the rail's foot that does something rather than open a
    /// place.
    fn action_tile(
        self,
        id: &'static str,
        icon: Icon,
        tip: &'static str,
        message: Message,
    ) -> Element<'a, Message> {
        let (width, height) = self.tile_size();
        let pointed = self.cx.pointed.rail.as_deref() == Some(id);
        let face = self.glyph_tile(icon, pointed);
        let marks = Marks {
            pointed,
            ..Marks::default()
        };
        let cell = self.tile_cell(
            id.into(),
            tip.into(),
            face,
            (width, height, 0.0),
            marks,
            message,
        );
        let cell: Element<'a, Message> = if id == "+"
            && let Some(foot) = self.tiles().and_then(|tiles| tiles.foot.as_ref())
            && let Some(art) = crate::theme_art::tile_art(
                self.cx,
                yapfast_core::themes::package::art::ArtSurface::Rail,
                foot,
                self.rail_width(),
                height + self.label_height(),
            ) {
            stack![cell, art]
                .width(self.rail_width())
                .height(height + self.label_height())
                .into()
        } else {
            cell
        };
        container(cell).height(height + self.label_height()).into()
    }

    /// A server's or a DM's face: its picture, else its initials on the
    /// raised ground (the accent while lit); faded while out of reach.
    fn server_face(self, item: &RailItem, lit: bool) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let (width, height) = self.tile_size();
        let radius = self.tile_radius();
        let picture_width = width.round() as u32;
        let picture_height = height.round() as u32;
        let opacity = if item.unavailable { 0.5 } else { 1.0 };
        if let Some(picture) = item
            .icon
            .as_deref()
            .and_then(|url| self.cx.picture(url, picture_width, picture_height, true))
        {
            return Pic::new(picture, Size::new(width, height))
                .radius(radius)
                .opacity(opacity)
                .into();
        }
        let (ground, ink) = if lit {
            (theme.accent, theme.on_accent)
        } else {
            (theme.raised, theme.text)
        };
        let (ground, ink) = (
            crate::style::faded(ground, opacity),
            crate::style::faded(ink, opacity),
        );
        container(text(tile_initials(item)).size(16).font(MEDIUM).color(ink))
            .center_x(width)
            .center_y(height)
            .style(move |_| {
                let mut style = crate::style::rounded(ground, radius);
                style.border = theme.surface_border("rail-icon", RADIUS, 0.0, Color::TRANSPARENT);
                style.border.radius = radius.into();
                style.shadow = theme.surface_shadow("rail-icon", iced::Shadow::default());
                style
            })
            .into()
    }

    /// A closed folder's face: its colour, with its first four servers'
    /// icons in a square; open, the folder's own icon in its colour.
    fn folder_face(
        self,
        open: bool,
        tint: u32,
        preview: &[(String, Option<String>)],
        pointed: bool,
    ) -> Element<'a, Message> {
        let tint = hex(tint);
        let theme = self.cx.look.theme;
        let (width, height) = self.folder_size();
        let radius = self.tile_radius();
        if open {
            return self
                .cx
                .look
                .glyph(Icon::Folder, 24, tint)
                .boxed(Length::Fill)
                .width(width)
                .height(height)
                .into();
        }
        let small = self
            .tiles()
            .and_then(|tiles| tiles.folder_mini)
            .unwrap_or_else(|| (width.min(height) - 10.0).max(16.0) / 2.0);
        let step = small + 2.0;
        let start_x = (width - (small * 2.0 + 2.0)) / 2.0;
        let start_y = (height - (small * 2.0 + 2.0)) / 2.0;
        let raised = self.cx.look.theme.raised;
        let mut faces = stack![];
        for (index, (initial, icon)) in preview.iter().enumerate() {
            // Each one's outer corner is round, as the folder's.
            let mut radius = [4.0; 4];
            radius[[0, 1, 3, 2][index]] = 13.0;
            let face: Element<'a, Message> = match icon
                .as_deref()
                .and_then(|url| self.cx.picture(url, small as u32, small as u32, true))
            {
                Some(picture) => Pic::square(picture, small).radius_each(radius).into(),
                None => container(
                    text(initial.clone())
                        .size(8)
                        .font(BOLD)
                        .color(self.cx.look.theme.text),
                )
                .center(small)
                .style(move |_| rounded(raised, corners(radius)))
                .into(),
            };
            let (x, y) = (
                start_x + (index % 2) as f32 * step,
                start_y + (index / 2) as f32 * step,
            );
            faces = faces.push(controls::placed(face, x, y));
        }
        let ground = Color {
            a: if pointed { 0.55 } else { 0.4 },
            ..tint
        };
        container(faces)
            .width(width)
            .height(height)
            .style(move |_| {
                let mut style = crate::style::rounded(ground, radius);
                style.border =
                    theme.surface_border("rail-icon", FOLDER_RADIUS, 0.0, Color::TRANSPARENT);
                style.border.radius = radius.into();
                style.shadow = theme.surface_shadow("rail-icon", iced::Shadow::default());
                style
            })
            .into()
    }

    fn rail_row(self, index: usize, item: &'a RailItem, height: f32) -> Element<'a, Message> {
        let tiles = self.tiles();
        let rail_width = if self.top_rail() {
            self.tile_size().0.max(self.folder_size().0).max(40.0) + 20.0
        } else {
            self.rail_width()
        };
        let (tile_width, tile_height) = self.tile_size();
        let (folder_width, folder_height) = self.folder_size();
        let pointed = self.cx.pointed.rail.as_deref() == Some(item.id.as_str());
        if item.kind == Kind::Divider {
            return if index == 0 {
                Space::new().into()
            } else {
                let (width, height, colour, dotted) = tiles.and_then(|tiles| tiles.divider).map_or(
                    (32.0, 1.0, self.cx.look.theme.outline, false),
                    |divider| {
                        (
                            divider.size.0,
                            divider.size.1,
                            crate::themes::argb(divider.colour),
                            divider.dotted,
                        )
                    },
                );
                let divider: Element<'a, Message> = if dotted {
                    let count = (width / (2.0 * height)).floor().max(1.0) as usize;
                    let mut dots = row![].spacing(height);
                    for _ in 0..count {
                        dots = dots.push(
                            container(Space::new())
                                .width(height)
                                .height(height)
                                .style(move |_| rounded(colour, height / 2.0)),
                        );
                    }
                    dots.into()
                } else {
                    bar(colour, width, height).into()
                };
                controls::placed(divider, (rail_width - width) / 2.0, ABOVE)
            };
        }
        let lit = item.selected || pointed;
        let (face, side, message) = match &item.kind {
            Kind::Folder {
                open,
                tint,
                preview,
            } => (
                self.folder_face(*open, *tint, preview, pointed),
                (folder_width, folder_height),
                Message::Rail(Event::Folder(item.id.clone())),
            ),
            Kind::Dm => (
                self.server_face(item, lit),
                (tile_width, tile_height),
                Message::Rail(Event::Dm(item.id.clone())),
            ),
            _ => (
                self.server_face(item, lit),
                (tile_width, tile_height),
                Message::Rail(Event::Server(item.id.clone())),
            ),
        };
        let place = self.s.items[..index]
            .iter()
            .filter(|i| i.kind != Kind::Divider)
            .count();
        let face = crate::theme_effects::rail_icon(face, self.cx, false, place);
        let face = crate::widgets::motion::interactive_cued(
            face,
            self.cx.look.theme.motion(),
            self.cx.look.theme.accent,
            self.cx.look.theme.surface_radius("rail-icon", RADIUS),
            self.cx.look.theme.has_hover_cue(),
            self.cx.look.theme.has_press_cue(),
        );
        let open_folder = matches!(item.kind, Kind::Folder { open: true, .. });
        let marks = Marks {
            selected: item.selected,
            unread: item.unread,
            badge: if open_folder { "" } else { &item.badge },
            call: item.call && !open_folder,
            pointed,
            folder: matches!(item.kind, Kind::Folder { .. }),
            open_folder,
            menu: true,
        };
        // A server out of reach in a Discord outage says so, besides
        // fading.
        let tip = if item.unavailable {
            Cow::Owned(format!("{} (unavailable in a Discord outage)", item.name))
        } else {
            Cow::Borrowed(item.name.as_str())
        };
        let cell = self.tile_cell(
            item.id.clone(),
            tip,
            face,
            (side.0, side.1, ABOVE),
            marks,
            message,
        );
        // An open folder's servers sit on a strip of its colour, from the
        // folder down to its last server.
        let strip = match &item.kind {
            Kind::Folder {
                open: true, tint, ..
            } => Some((*tint, true, false)),
            Kind::Nested { tint, last } => Some((*tint, false, *last)),
            _ => None,
        };
        let mut layers = stack![];
        if index > 0
            && let Some(tiles) = tiles
            && let Some(between) = tiles.between.as_ref()
            && let Some(art) = crate::theme_art::tile_art(
                self.cx,
                yapfast_core::themes::package::art::ArtSurface::Rail,
                between,
                rail_width,
                tiles.between_height,
            )
        {
            let gap = tiles.gap + 2.0 * tiles.pad;
            layers = layers.push(controls::placed(
                art,
                0.0,
                (gap - tiles.between_height).max(0.0) / 2.0,
            ));
        }
        if let Some((tint, top, last)) = strip {
            let ground = Color {
                a: 0.16,
                ..hex(tint)
            };
            let round = |on: bool| if on { RADIUS } else { 0.0 };
            let shape = [round(top), round(top), round(last), round(last)];
            let strip_width = tiles.map_or(FOLDER, |tiles| tiles.width);
            let strip_height = if last { tile_height + 4.0 } else { height };
            layers = layers.push(controls::placed(
                container(Space::new())
                    .width(strip_width)
                    .height(strip_height)
                    .style(move |_| rounded(ground, corners(shape))),
                (rail_width - strip_width) / 2.0,
                ABOVE,
            ));
        }
        layers
            .push(cell)
            .width(rail_width)
            .height(height + ABOVE)
            .into()
    }

    /// A tile in its cell across the rail, `above` px under the cell's top:
    /// the pill at the rail's edge, the tile under the pointer's watch, its
    /// optional owner-theme label, badge and speaker, and its tooltip.
    fn tile_cell(
        self,
        id: String,
        tip: Cow<'a, str>,
        face: Element<'a, Message>,
        (tile_width, tile_height, above): (f32, f32, f32),
        marks: Marks<'a>,
        message: Message,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let tiles = self.tiles();
        let top_rail = self.top_rail();
        let rail_width = if top_rail {
            self.tile_size().0.max(self.folder_size().0).max(40.0) + 20.0
        } else {
            self.rail_width()
        };
        let right_rail = crate::conversation::navigation_right(
            self.cx.themes.package_layout(&self.cx.look.theme.id),
        );
        let edge_direction = if right_rail { 1.0 } else { -1.0 };
        let selected_offset = tiles
            .filter(|_| marks.selected)
            .map_or((0.0, 0.0), |tiles| tiles.selected.offset);
        let hover_lift = tiles.map_or(0.0, |tiles| {
            if marks.pointed && tiles.hover == yapfast_core::themes::lists::HoverKind::Lift {
                tiles.hover_amount
            } else {
                0.0
            }
        });
        let left = (rail_width - tile_width) / 2.0 + selected_offset.0 * edge_direction;
        let tile_top = if top_rail {
            let height = themed_top_height(self.cx.themes.package_layout(&theme.id));
            (height - tile_height - self.label_height()).max(0.0) / 2.0 + selected_offset.1
                - hover_lift
        } else {
            above + selected_offset.1 - hover_lift
        };
        let pill_allowed = tiles.is_none_or(|tiles| match tiles.pill {
            PillMode::On => true,
            PillMode::Off => false,
            PillMode::Unread => marks.unread,
        });
        let pill = if !pill_allowed || marks.open_folder {
            0.0
        } else if marks.selected {
            PILL_OPEN
        } else if marks.pointed {
            PILL_POINTED
        } else if marks.unread {
            PILL_UNREAD
        } else {
            0.0
        };
        let ink = theme.token_colour("pill", theme.text);
        let mut layers = stack![];
        if pill > 0.0 {
            let (pill_x, pill_y, pill_width, pill_height, pill_corners) = if top_rail {
                (
                    left + (tile_width - pill) / 2.0,
                    themed_top_height(self.cx.themes.package_layout(&theme.id)) - 4.0,
                    pill,
                    4.0,
                    [4.0, 4.0, 0.0, 0.0],
                )
            } else {
                (
                    if right_rail { rail_width - 4.0 } else { 0.0 },
                    tile_top + (tile_height - pill) / 2.0,
                    4.0,
                    pill,
                    if right_rail {
                        [4.0, 0.0, 0.0, 4.0]
                    } else {
                        [0.0, 4.0, 4.0, 0.0]
                    },
                )
            };
            let pill_el: Element<'a, Message> = container(Space::new())
                .width(pill_width)
                .height(pill_height)
                .style(move |_| rounded(ink, corners(pill_corners)))
                .into();
            let pill_el = if marks.unread && !marks.selected && !marks.pointed {
                crate::theme_effects::slot(
                    pill_el,
                    self.cx,
                    yapfast_core::themes::package::Slot::Unread,
                    Some(theme.rail),
                )
            } else {
                pill_el
            };
            layers = layers.push(controls::placed(pill_el, pill_x, pill_y));
        }
        let label = crate::themes::list_case(tiles.map(|tiles| tiles.label_case), tip.as_ref())
            .into_owned();
        let label_ink = tiles
            .and_then(|tiles| tiles.label_colour)
            .map(crate::themes::argb)
            .unwrap_or(theme.text);
        let label_ground = tiles
            .and_then(|tiles| tiles.label_ground)
            .map(crate::themes::argb);
        let label_size = tiles.map_or(8.0, |tiles| tiles.label_size);
        let label_box = tiles.and_then(|tiles| tiles.label_box);
        let label_font_role = tiles.map_or("small", |tiles| tiles.label_font.name());
        let label_font = theme.font(label_font_role, MEDIUM);
        let label_view = move |width: f32, height: f32| {
            container(
                text(label.clone())
                    .size(label_size)
                    .font(label_font)
                    .color(label_ink)
                    .wrapping(text::Wrapping::None),
            )
            .width(width)
            .height(height)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                background: label_ground.map(Into::into),
                ..Default::default()
            })
        };
        let mut tile_face = stack![].width(tile_width).height(tile_height);
        if let Some(below) = tiles.and_then(|tiles| tiles.below.as_ref())
            && let Some(art) = crate::theme_art::tile_art(
                self.cx,
                yapfast_core::themes::package::art::ArtSurface::Rail,
                below,
                tile_width,
                tile_height,
            )
        {
            tile_face = tile_face.push(art);
        }
        tile_face = tile_face.push(face);
        if let Some(frame) = tiles.and_then(|tiles| {
            if marks.folder {
                if marks.open_folder {
                    tiles.folder_open_frame.as_ref()
                } else {
                    tiles.folder_frame.as_ref()
                }
            } else if marks.selected {
                tiles.selected.frame.as_ref()
            } else {
                None
            }
            .or_else(|| crate::theme_art::picked_tile(&tiles.frame, &id))
        }) && let Some(art) = crate::theme_art::tile_art(
            self.cx,
            yapfast_core::themes::package::art::ArtSurface::Rail,
            frame,
            tile_width,
            tile_height,
        ) {
            tile_face = tile_face.push(art);
        }
        if tiles.is_some_and(|tiles| tiles.label == LabelPlace::Inside) {
            let [x, y, width, height] = label_box.unwrap_or([
                2.0,
                (tile_height - label_size - 4.0).max(0.0),
                (tile_width - 4.0).max(1.0),
                label_size + 4.0,
            ]);
            tile_face = tile_face.push(controls::placed(label_view(width, height), x, y));
        }
        let below_label = tiles.is_some_and(|tiles| tiles.label == LabelPlace::Below);
        let label_height = if below_label { label_size + 2.0 } else { 0.0 };
        let mut tile_face = stack![tile_face]
            .width(tile_width)
            .height(tile_height + label_height);
        if below_label {
            tile_face = tile_face.push(controls::placed(
                label_view(tile_width, label_height),
                0.0,
                tile_height,
            ));
        }
        if let Some(over) = tiles.and_then(|tiles| {
            if marks.selected {
                tiles.selected.over.as_ref()
            } else {
                None
            }
            .or_else(|| crate::theme_art::picked_tile(&tiles.over, &id))
        }) && let Some(art) = crate::theme_art::tile_art(
            self.cx,
            yapfast_core::themes::package::art::ArtSurface::Rail,
            over,
            tile_width,
            tile_height + label_height,
        ) {
            tile_face = tile_face.push(art);
        }
        let tile = mouse_area(tile_face)
            .interaction(iced::mouse::Interaction::Pointer)
            .on_enter(Message::Rail(Event::Point(id.clone(), true)))
            .on_exit(Message::Rail(Event::Point(id.clone(), false)))
            .on_press(message);
        // Where in the window it was pressed: the rail scrolls.
        let tile: Element<'a, Message> = if marks.menu {
            let spot = self.cx.pressed;
            RightPress::new(tile, move |_| {
                Message::Rail(Event::Menu(id.clone(), spot.get()))
            })
            .into()
        } else {
            tile.into()
        };
        let position = if top_rail {
            tooltip::Position::Bottom
        } else if right_rail {
            tooltip::Position::Left
        } else {
            tooltip::Position::Right
        };
        let tile = tooltip(tile, controls::tip(theme, tip), position).gap(8);
        layers = layers.push(controls::placed(tile, left, tile_top));
        if marks.call {
            layers = layers.push(controls::placed(
                self.call_bubble(),
                left + tile_width + 3.0 - 20.0,
                tile_top - 3.0,
            ));
        }
        if !marks.badge.is_empty() {
            // Its right and bottom edges 4 px past the tile's.
            let badge = match tiles {
                Some(tiles)
                    if tiles.badge.ground.is_some()
                        || tiles.badge.ink.is_some()
                        || tiles.badge.radius.is_some() =>
                {
                    self.mention_badge(marks.badge, tiles.badge)
                }
                _ => count_pill(theme, marks.badge, RAIL_COUNT, Some(theme.rail)),
            };
            layers = layers.push(
                container(crate::theme_effects::slot(
                    badge,
                    self.cx,
                    yapfast_core::themes::package::Slot::Mention,
                    Some(theme.rail),
                ))
                .width(left + tile_width + BELOW)
                .height(tile_top + tile_height + BELOW)
                .align_x(Alignment::End)
                .align_y(Alignment::End),
            );
        }
        layers
            .width(rail_width)
            .height(if top_rail {
                themed_top_height(self.cx.themes.package_layout(&theme.id))
            } else {
                tile_top + tile_height + label_height + BELOW
            })
            .into()
    }

    fn mention_badge(
        self,
        count: &str,
        badge: yapfast_core::themes::lists::MentionBadge,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let ground = badge
            .ground
            .map(crate::themes::argb)
            .unwrap_or(theme.danger);
        let ink = badge
            .ink
            .map(crate::themes::argb)
            .unwrap_or(theme.on_danger);
        let radius = badge.radius.unwrap_or(RAIL_COUNT.height / 2.0);
        let number = text(count.to_owned())
            .size(RAIL_COUNT.size)
            .font(BOLD)
            .color(ink)
            .wrapping(text::Wrapping::None);
        let mut pill = container(number)
            .padding([0.0, RAIL_COUNT.pad])
            .height(RAIL_COUNT.height)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);
        if count.chars().count() < 2 {
            pill = pill.width(RAIL_COUNT.height);
        }
        let pill = pill.style(move |_| rounded(ground, radius));
        container(pill)
            .padding(3)
            .style(move |_| rounded(theme.rail, radius + 3.0))
            .into()
    }

    /// A call going on: a white speaker on green, on a ring of the rail's
    /// colour, with a dark copy a pixel lower as its shadow, which the light
    /// green needs at this size.
    fn call_bubble(self) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let (rail, live) = (theme.rail, theme.live);
        let shadow = Color {
            a: 0.45,
            ..Color::BLACK
        };
        let speaker = stack![
            controls::placed(self.cx.look.glyph(Icon::Speaker, 12, shadow), 0.0, 1.0),
            self.cx.look.glyph(Icon::Speaker, 12, Color::WHITE),
        ];
        container(
            container(speaker)
                .center(16)
                .style(move |_| rounded(live, 8)),
        )
        .center(20)
        .style(move |_| rounded(rail, 10))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: Kind, selected: bool) -> RailItem {
        RailItem {
            id: id.into(),
            name: id.into(),
            icon: None,
            kind,
            selected,
            unread: false,
            badge: String::new(),
            call: false,
            unavailable: false,
        }
    }

    #[test]
    fn a_tree_puts_channels_after_the_selected_server_or_its_closed_folder() {
        let divider = item("divider", Kind::Divider, false);
        let server = item("server", Kind::Server, true);
        assert_eq!(tree_at(&[divider.clone(), server]), Some(1));

        let folder = item(
            "folder",
            Kind::Folder {
                open: false,
                tint: 0,
                preview: Vec::new(),
            },
            true,
        );
        assert_eq!(tree_at(&[divider.clone(), folder]), Some(1));

        let open = item(
            "folder",
            Kind::Folder {
                open: true,
                tint: 0,
                preview: Vec::new(),
            },
            true,
        );
        let nested = item(
            "server",
            Kind::Nested {
                tint: 0,
                last: true,
            },
            true,
        );
        assert_eq!(tree_at(&[divider.clone(), open, nested]), Some(2));

        let dm = item("dm", Kind::Dm, true);
        assert_eq!(tree_at(&[dm, divider]), None);
    }
}
