//! A server's rows: headings, links, channels with their threads under
//! them, and their faces and inks.

use iced::{
    Alignment, Color, Element, Length, Padding,
    widget::{Space, column, container, mouse_area, row, stack, text},
};

use crate::{
    app::Message,
    controls::{self, ROW_COUNT, bar, count_pill},
    elide::elide,
    fonts::{BOLD, MEDIUM, REGULAR, SEMIBOLD},
    pictures::Icon,
    style::{corners, faded, rounded},
    widgets::{Drags, RightPress},
};

use super::{CHAT_BUTTON, Event, GAP, HEADING, INSET, ROW, Room, SideRow, VOICE_CARD_GAP};

fn themed_count_pill<'a>(
    theme: &crate::theme::Theme,
    count: &str,
    badge: yapfast_core::themes::lists::MentionBadge,
) -> Element<'a, Message> {
    if badge.ground.is_none() && badge.ink.is_none() && badge.radius.is_none() {
        return count_pill(theme, count, ROW_COUNT, None);
    }
    let ground = badge
        .ground
        .map(crate::themes::argb)
        .unwrap_or(theme.danger);
    let ink = badge
        .ink
        .map(crate::themes::argb)
        .unwrap_or(theme.on_danger);
    let radius = badge.radius.unwrap_or(ROW_COUNT.height / 2.0);
    let number = text(count.to_owned())
        .size(ROW_COUNT.size)
        .font(BOLD)
        .color(ink)
        .wrapping(text::Wrapping::None);
    let mut pill = container(number)
        .padding([0.0, ROW_COUNT.pad])
        .height(ROW_COUNT.height)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    if count.chars().count() < 2 {
        pill = pill.width(ROW_COUNT.height);
    }
    pill.style(move |_| rounded(ground, radius)).into()
}

impl<'a> super::View<'a> {
    pub(super) fn side_row(self, row: &'a SideRow, place: Option<usize>) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let row_gap = self
            .cx
            .themes
            .channel_rows(&theme.id)
            .and_then(|rows| rows.gap)
            .unwrap_or(GAP);
        let inner: Element<'a, Message> = match row {
            SideRow::Screens {
                friends,
                requests,
                badge,
            } => column![
                self.link_row(
                    "screen:friends",
                    Icon::Users,
                    "Friends",
                    *friends,
                    "",
                    Message::Side(Event::Friends)
                ),
                self.link_row(
                    "screen:requests",
                    Icon::Inbox,
                    "Requests",
                    *requests,
                    badge,
                    Message::Side(Event::Requests)
                ),
            ]
            .spacing(GAP)
            .padding([6, 0])
            .into(),
            SideRow::DmHeading => row![
                text("Direct Messages")
                    .size(theme.type_size("labels", 12.0))
                    .font(theme.font("labels", SEMIBOLD))
                    .color(theme.muted)
                    .width(Length::Fill),
                self.cx
                    .look
                    .icon_button(
                        Icon::UserPlus,
                        "Add a friend",
                        Message::Friends(crate::friends::Event::OpenAdd)
                    )
                    .size(24.0, 16)
                    .below(),
                self.cx
                    .look
                    .icon_button(
                        Icon::SquarePen,
                        "New message",
                        Message::Switcher(crate::switcher::Event::OpenNewDm)
                    )
                    .size(24.0, 16)
                    .below(),
            ]
            .spacing(4)
            .padding(Padding {
                left: 8.0,
                ..Padding::ZERO
            })
            .height(40)
            .align_y(Alignment::Center)
            .into(),
            SideRow::Browse { open } => container(self.link_row(
                "screen:browse",
                Icon::Hash,
                "Browse channels",
                *open,
                "",
                Message::Side(Event::Browse),
            ))
            .padding([6, 0])
            .into(),
            SideRow::Pinned => container(
                text("PINNED CHANNELS")
                    .size(theme.type_size("labels", 12.0))
                    .font(theme.font("labels", BOLD))
                    .color(theme.muted),
            )
            .padding(Padding {
                left: 16.0,
                ..Padding::ZERO
            })
            .height(36)
            .align_y(Alignment::Center)
            .into(),
            SideRow::PinnedEnd => container(bar(theme.line, Length::Fill, 1))
                .height(9)
                .align_y(Alignment::Center)
                .into(),
            SideRow::Heading {
                name,
                fold,
                collapsed,
            } => self.heading(name, fold, *collapsed, place),
            SideRow::Room(room) => self.room_rows(room, place),
            SideRow::Empty(words) => container(
                text(*words)
                    .size(theme.type_size("body", 14.0))
                    .font(theme.font("body", REGULAR))
                    .color(theme.muted),
            )
            .padding(8)
            .into(),
        };
        let inset = container(inner)
            .padding(Padding {
                left: INSET,
                right: INSET,
                bottom: row_gap,
                top: 0.0,
            })
            .width(Length::Fill);
        let stripe = match row {
            SideRow::Room(_) => place.and_then(|index| {
                theme
                    .surface_stripe("row")
                    .filter(|stripe| stripe.channels && index % 2 == 1)
                    .map(|stripe| stripe.colour)
            }),
            _ => None,
        };
        let inset: Element<'a, Message> = match stripe {
            Some(ground) => container(inset)
                .width(Length::Fill)
                .style(move |_| crate::style::fill(ground))
                .into(),
            None => inset.into(),
        };
        // A row that may be dragged: where it would land marked by a line
        // across its top (dragged up) or its foot (dragged down).
        let inset: Element<'a, Message> = match self.s.drop_line(place) {
            Some(below) => {
                // From 8 px into the row to its end.
                let line = container(self.drop_line_view()).padding(Padding {
                    left: 2.0 * INSET,
                    right: INSET,
                    ..Padding::ZERO
                });
                let y = if below {
                    row.height(theme, row_gap) - crate::arrange::LINE
                } else {
                    0.0
                };
                stack![inset, controls::placed(line, 0.0, y)].into()
            }
            None => inset,
        };
        // Where someone dragged from a voice channel would drop: outlined
        // in the drop line's green.
        let inset: Element<'a, Message> = match row {
            SideRow::Room(room) if self.s.move_aimed(&room.id) => {
                let live = theme.live;
                let outline = container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(move |_| iced::widget::container::Style {
                        border: iced::Border {
                            color: live,
                            width: 2.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    });
                stack![
                    inset,
                    container(outline).padding(Padding {
                        left: INSET,
                        right: INSET,
                        bottom: GAP,
                        top: 0.0,
                    })
                ]
                .into()
            }
            _ => inset,
        };
        // While someone is dragged, a voice channel's row says when the
        // pointer comes onto it, a folded category's busy one too.
        let onto = match row {
            SideRow::Room(room) if room.voice => Some(room.id.as_str()),
            _ => None,
        };
        let moving = self.s.moving_person.is_some();
        let over = move |place: Option<usize>, over: bool| match (onto, place) {
            (Some(room), _) if moving => {
                Message::Arrange(crate::arrange::Event::Onto(room.to_owned(), over))
            }
            (_, Some(place)) if !moving => {
                Message::Arrange(crate::arrange::Event::Over(place, over))
            }
            _ => Message::Nothing,
        };
        let inset: Element<'a, Message> = match place {
            Some(place) => {
                let click = match row {
                    SideRow::Heading { fold, .. } => Message::Side(Event::Toggle(fold.clone())),
                    SideRow::Room(room) if room.voice => {
                        Message::Side(Event::Voice(room.id.clone()))
                    }
                    SideRow::Room(room) => Message::Side(Event::Select(room.id.clone())),
                    _ => return inset,
                };
                let head = match row {
                    SideRow::Room(room) => room.row_height(theme),
                    _ => HEADING,
                };
                Drags::new(inset)
                    .click(click)
                    .head(head)
                    .drag(move |grab| Message::Arrange(crate::arrange::Event::Start(place, grab)))
                    .tracking(self.s.dragging(), move |on| over(Some(place), on))
                    .into()
            }
            None if onto.is_some() => Drags::new(inset)
                .tracking(moving, move |on| over(None, on))
                .into(),
            None => inset,
        };
        let inset = match row {
            // Unread activity is marked at the sidebar's edge, as on the
            // rail, a thread's too.
            SideRow::Room(room)
                if room.unread
                    && !room.selected
                    && self
                        .cx
                        .themes
                        .channel_rows(&theme.id)
                        .and_then(|rows| rows.meta.unread)
                        .is_none() =>
            {
                let ink = theme.text;
                let mark = container(Space::new())
                    .width(4)
                    .height(8)
                    .style(move |_| rounded(ink, corners([0.0, 4.0, 4.0, 0.0])));
                stack![
                    inset,
                    controls::placed(mark, 0.0, (room.row_height(theme) - 8.0) / 2.0)
                ]
                .into()
            }
            _ => inset,
        };
        if matches!(row, SideRow::Room(_))
            && let Some(separator) = self
                .cx
                .themes
                .channel_rows(&theme.id)
                .and_then(|rows| rows.separator.as_ref())
        {
            let row_height = row.height(theme, row_gap);
            let separator: Option<(Element<'a, Message>, f32)> = match separator {
                yapfast_core::themes::lists::Separator::Line {
                    colour,
                    width,
                    inset: (left, right),
                } => Some((
                    container(bar(crate::themes::argb(*colour), Length::Fill, *width))
                        .padding(Padding {
                            left: *left,
                            right: *right,
                            ..Padding::ZERO
                        })
                        .into(),
                    *width,
                )),
                yapfast_core::themes::lists::Separator::Art { art, height } => {
                    crate::theme_art::tile_art(
                        self.cx,
                        yapfast_core::themes::package::art::ArtSurface::Sidebar,
                        art,
                        super::width(self.cx.layout, theme),
                        *height,
                    )
                    .map(|art| (art, *height))
                }
            };
            if let Some((separator, height)) = separator {
                return stack![
                    inset,
                    controls::placed(separator, 0.0, (row_height - row_gap - height).max(0.0),)
                ]
                .into();
            }
        }
        inset
    }

    /// A category's heading: 17 px above its name, the chevron after it
    /// turned while folded; a click folds it or opens it, and a Discord
    /// category's opens its menu on a right click.
    pub(super) fn heading(
        self,
        name: &'a str,
        fold: &'a str,
        collapsed: bool,
        place: Option<usize>,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let name = self.cx.themes.label(
            &theme.id,
            yapfast_core::themes::chrome::Label::Category,
            name,
            &[("name", name)],
        );
        let rows = self.cx.themes.channel_rows(&theme.id);
        let headers = rows.map(|rows| &rows.headers);
        let place_text = place.map_or_else(String::new, |place| (place + 1).to_string());
        let name = if let Some(headers) = headers.filter(|headers| !headers.text.is_empty()) {
            yapfast_core::themes::chrome::fill(
                &headers.text,
                &[("name", name.as_str()), ("n", place_text.as_str())],
            )
        } else {
            name
        };
        let name = crate::themes::list_case(headers.and_then(|headers| headers.case), &name);
        let name = theme.surface_words("label", &name).into_owned();
        let id = format!("heading:{fold}");
        let pointed = self.cx.pointed.side.as_deref() == Some(id.as_str());
        let stock_ink = if pointed {
            theme.secondary
        } else {
            theme.muted
        };
        let ink = headers
            .and_then(|headers| headers.colour)
            .map(crate::themes::argb)
            .unwrap_or(stock_ink);
        // While a drag is on, as a text channel's row.
        let ink = if self.s.arrange_dimmed(None, false) {
            faded(ink, crate::arrange::DIM)
        } else {
            ink
        };
        let header_font = crate::themes::list_font(
            theme,
            headers.and_then(|h| h.font),
            "labels",
            headers.and_then(|h| h.weight),
            SEMIBOLD,
        );
        let header_shadow = crate::themes::offset_shadow(headers.and_then(|h| h.shadow));
        let mut words = row![
            elide(name)
                .size(
                    headers
                        .and_then(|headers| headers.size)
                        .unwrap_or_else(|| theme.type_size("labels", 14.0)),
                )
                .font(header_font)
                .color(ink)
                .light(header_shadow)
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        if headers.and_then(|headers| headers.chevron) != Some(false) {
            words = words.push(self.cx.look.glyph(
                if collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                },
                12,
                ink,
            ));
        }
        let face = words;
        let mut face = row![face, Space::new().width(Length::Fill)].align_y(Alignment::Center);
        // Discord's "+", while the pointer is over a category this account
        // may make channels in.
        if pointed && self.cx.app.may_create_channel(fold) {
            face = face.push(
                self.cx
                    .look
                    .icon_button(
                        Icon::Add,
                        "Create Channel",
                        Message::Side(Event::CreateIn(fold.to_owned())),
                    )
                    .size(CHAT_BUTTON, 16),
            );
        }
        let border = theme.surface_border("label", 0.0, 0.0, Color::TRANSPARENT);
        let shadow = theme.surface_shadow("label", iced::Shadow::default());
        let surface = yapfast_core::themes::package::art::ArtSurface::Label;
        let decorated = self.cx.themes.decorates(&theme.id, surface);
        let background = (!decorated)
            .then(|| theme.surface_ground("label"))
            .flatten();
        let height = headers
            .and_then(|headers| headers.height)
            .unwrap_or(HEADING);
        let fill_width = headers.is_some_and(|h| h.fill);
        let inset_left = headers
            .and_then(|headers| headers.inset)
            .unwrap_or(if fill_width { 6.0 } else { 16.0 });
        let body: Element<'a, Message> = container(face)
            .padding(Padding {
                left: inset_left,
                right: if fill_width { 6.0 } else { 8.0 },
                top: if fill_width { 4.0 } else { 17.0 },
                bottom: 0.0,
            })
            .height(height)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                background,
                border,
                shadow,
                ..Default::default()
            })
            .into();
        let body: Element<'a, Message> = match headers.and_then(|headers| headers.frame.as_ref()) {
            Some(frame) => match crate::theme_art::tile_art(
                self.cx,
                yapfast_core::themes::package::art::ArtSurface::Sidebar,
                frame,
                super::width(self.cx.layout, theme),
                height,
            ) {
                Some(art) => stack![art, body].width(Length::Fill).height(height).into(),
                None => body,
            },
            None => body,
        };
        let body = crate::theme_art::surface(body, self.cx, surface);
        let area = mouse_area(body)
            .interaction(iced::mouse::Interaction::Pointer)
            .on_enter(Message::Side(Event::Point(id.clone(), true)))
            .on_exit(Message::Side(Event::Point(id, false)));
        // One that may be dragged folds on a click, not a press (`Drags`).
        let area = if place.is_none() {
            area.on_press(Message::Side(Event::Toggle(fold.to_owned())))
        } else {
            area
        };
        if !self.cx.state.is_category(fold) {
            return area.into();
        }
        // Where in the window it was pressed: the list's own space is under
        // the header, and scrolled.
        let (room, spot) = (fold.to_owned(), self.cx.pressed);
        RightPress::new(area, move |_| {
            Message::Side(Event::Menu(room.clone(), spot.get()))
        })
        .into()
    }

    /// A row that opens one of the DM home's screens or Browse Channels.
    pub(super) fn link_row(
        self,
        id: &'static str,
        icon: Icon,
        label: &'static str,
        selected: bool,
        badge: &str,
        message: Message,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let pointed = self.cx.pointed.side.as_deref() == Some(id);
        let ink = if selected {
            theme.text
        } else if pointed {
            theme.secondary
        } else {
            theme.muted
        };
        let icon_ink = if selected { theme.text } else { theme.faint };
        let mut face = row![
            self.cx.look.glyph(icon, 20, icon_ink),
            text(label)
                .size(theme.type_size("body", 15.0))
                .font(theme.font("body", MEDIUM))
                .color(ink)
                .width(Length::Fill)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if !badge.is_empty() {
            face = face.push(count_pill(theme, badge, ROW_COUNT, None));
        }
        self.row_face(id.into(), face.into(), ROW, selected, Some(message), None)
    }

    /// A row's face: lit while selected or pointed at, a press (unless a
    /// click is the row's `Drags`) and a right click on it.
    pub(super) fn row_face(
        self,
        id: String,
        face: Element<'a, Message>,
        height: f32,
        selected: bool,
        message: Option<Message>,
        menu: Option<String>,
    ) -> Element<'a, Message> {
        let theme = self.cx.look.theme;
        let pointed = self.cx.pointed.side.as_deref() == Some(id.as_str());
        let rows = self.cx.themes.channel_rows(&theme.id);
        let frame = rows.and_then(|rows| {
            if selected {
                rows.selected.frame.as_ref()
            } else {
                None
            }
            .or_else(|| crate::theme_art::picked_tile(&rows.frame, &id))
        });
        let selected_plain =
            frame.is_none() && rows.is_none_or(|rows| rows.selected.underline.is_none());
        let selected_surface = yapfast_core::themes::package::art::ArtSurface::RowSelected;
        let selected_decorated =
            selected && selected_plain && self.cx.themes.decorates(&theme.id, selected_surface);
        let event_highlighter = selected
            && self
                .cx
                .themes
                .event_effect(
                    &theme.id,
                    yapfast_core::themes::package::art::EventSlot::Select,
                )
                .is_some();
        let ground = if selected {
            if event_highlighter {
                Color::TRANSPARENT
            } else if selected_plain {
                if selected_decorated {
                    Color::TRANSPARENT
                } else {
                    theme.token_colour("row-selected", theme.active)
                }
            } else {
                Color::TRANSPARENT
            }
        } else if pointed {
            match rows.and_then(|rows| rows.hover_ground).unwrap_or_else(|| {
                if rows.is_some_and(|rows| !rows.frame.is_empty()) {
                    yapfast_core::themes::lists::HoverGround::None
                } else {
                    yapfast_core::themes::lists::HoverGround::Hover
                }
            }) {
                yapfast_core::themes::lists::HoverGround::None => Color::TRANSPARENT,
                yapfast_core::themes::lists::HoverGround::Hover => theme.hover,
                yapfast_core::themes::lists::HoverGround::RowHover => theme.row_hover,
            }
        } else {
            Color::TRANSPARENT
        };
        let mut border = theme.surface_border("row", 6.0, 0.0, Color::TRANSPARENT);
        border.radius = theme.row_radius(6.0).into();
        let shadow = theme.surface_shadow("row", iced::Shadow::default());
        let inset = self
            .cx
            .themes
            .channel_rows(&theme.id)
            .and_then(|rows| rows.inset)
            .unwrap_or(8.0);
        let body: Element<'a, Message> = container(face)
            .padding([0.0, inset])
            .height(height)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .clip(true)
            .style(move |_| container::Style {
                background: Some(ground.into()),
                border,
                shadow,
                ..Default::default()
            })
            .into();
        let mut body = iced::widget::stack![body]
            .width(Length::Fill)
            .height(height);
        if let Some(frame) = frame
            && let Some(art) = crate::theme_art::tile_art_opacity(
                self.cx,
                yapfast_core::themes::package::art::ArtSurface::Sidebar,
                frame,
                super::width(self.cx.layout, theme),
                height,
                if pointed && !selected {
                    rows.and_then(|rows| rows.hover_frame_opacity)
                        .unwrap_or(1.0)
                } else {
                    1.0
                },
            )
        {
            body = iced::widget::stack![art, body]
                .width(Length::Fill)
                .height(height);
        }
        if selected
            && let Some(after) = rows.and_then(|rows| rows.selected.after.as_ref())
            && let Some(art) = crate::theme_art::tile_art(
                self.cx,
                yapfast_core::themes::package::art::ArtSurface::Sidebar,
                after,
                super::width(self.cx.layout, theme),
                height,
            )
        {
            body = body.push(art);
        }
        let body: Element<'a, Message> = body.into();
        let body = if selected && selected_plain {
            crate::theme_art::surface(body, self.cx, selected_surface)
        } else {
            body
        };
        let body = crate::theme_effects::selected_row(body, self.cx, selected);
        let area = mouse_area(body)
            .interaction(iced::mouse::Interaction::Pointer)
            .on_enter(Message::Side(Event::Point(id.clone(), true)))
            .on_exit(Message::Side(Event::Point(id, false)));
        let area = match message {
            Some(message) => area.on_press(message),
            None => area,
        };
        match menu {
            Some(room) => {
                let spot = self.cx.pressed;
                RightPress::new(area, move |_| {
                    Message::Side(Event::Menu(room.clone(), spot.get()))
                })
                .into()
            }
            None => area.into(),
        }
    }

    /// A conversation's row, with its thread line, and who is in it for a
    /// busy voice channel.
    pub(super) fn room_rows(self, room: &'a Room, place: Option<usize>) -> Element<'a, Message> {
        let line = self.room_row(room, place.is_some());
        let line: Element<'a, Message> = if room.thread {
            row![self.thread_line(room.thread_continues), line].into()
        } else {
            line
        };
        if room.people.is_empty() {
            return line;
        }
        let mut people = column![].padding(Padding {
            left: 32.0,
            bottom: 4.0,
            ..Padding::ZERO
        });
        // The call this account is in shows who speaks, in the call's
        // order: this account first.
        let voice = &self.cx.state.voice;
        let call = voice.active() && voice.room.as_deref() == Some(room.id.as_str());
        let mut listed: Vec<_> = room
            .people
            .iter()
            .map(|person| {
                let live = call
                    .then(|| {
                        voice
                            .participants
                            .iter()
                            .position(|live| live.person == person.id || live.id == person.id)
                    })
                    .flatten();
                (live, person)
            })
            .collect();
        if call {
            listed.sort_by_key(|(live, _)| live.unwrap_or(usize::MAX));
        }
        for (live, person) in listed {
            let live = live.map(|place| &voice.participants[place]);
            people = people.push(self.voice_person(&room.id, person, live));
        }
        column![line, people].spacing(GAP).into()
    }

    /// The line from a channel to its thread, 28 px wide: down from the
    /// channel and round into the thread, on down while another follows.
    pub(super) fn thread_line(self, continues: bool) -> Element<'a, Message> {
        let faint = self.cx.look.theme.faint;
        let down = bar(faint, 1.5, if continues { ROW + GAP } else { 13.0 + GAP });
        let round = container(Space::new())
            .width(10.75)
            .height(7.5)
            .style(move |_| iced::widget::container::Style {
                border: iced::Border {
                    color: faint,
                    width: 1.5,
                    radius: corners([0.0, 0.0, 0.0, 6.0]),
                },
                ..Default::default()
            });
        container(stack![
            controls::placed(down, 17.25, 0.0),
            controls::placed(round, 17.25, 12.75),
        ])
        .width(28)
        .height(ROW)
        .into()
    }

    /// A conversation's row; one that may be `dragged` is clicked through
    /// its `Drags`.
    pub(super) fn room_row(self, room: &'a Room, dragged: bool) -> Element<'a, Message> {
        if room.direct {
            return self.dm_row(room);
        }
        let theme = self.cx.look.theme;
        let list = self.cx.themes.channel_rows(&theme.id);
        let pointed = self.cx.pointed.side.as_deref() == Some(room.id.as_str());
        let (ink, opacity) = self.row_ink(room, pointed);
        // While a drag is on, rows it can't land among dim.
        let (ink, opacity) = if self.s.arrange_dimmed(Some(&room.id), room.voice) {
            (
                faded(ink, crate::arrange::DIM),
                opacity * crate::arrange::DIM,
            )
        } else {
            (ink, opacity)
        };
        let icon_size = list.and_then(|rows| rows.icon.size).unwrap_or(20.0);
        let icon_gap = list.and_then(|rows| rows.icon.gap).unwrap_or(8.0);
        let mut face = row![].spacing(icon_gap).align_y(Alignment::Center);
        if room.selected
            && let Some(prefix_art) = list.and_then(|rows| rows.selected.prefix_art.as_ref())
            && let Some(prefix) = crate::theme_art::tile_art(
                self.cx,
                yapfast_core::themes::package::art::ArtSurface::Sidebar,
                prefix_art,
                icon_size,
                icon_size,
            )
        {
            face = face.push(prefix);
        } else if room.selected
            && let Some(prefix) = list
                .map(|rows| rows.selected.prefix.as_str())
                .filter(|prefix| !prefix.is_empty())
        {
            let colour = list
                .and_then(|rows| rows.selected.prefix_colour)
                .map(crate::themes::argb)
                .unwrap_or(ink);
            face = face.push(text(prefix.to_owned()).size(icon_size).color(colour));
        }
        if !room.thread {
            let stock_icon = if room.selected {
                theme.text
            } else if room.unread {
                theme.secondary
            } else {
                theme.faint
            };
            let icon_ink = list.map_or(stock_icon, |rows| {
                if rows.icon.follow_ink {
                    ink
                } else if room.selected {
                    rows.icon
                        .selected
                        .map(crate::themes::argb)
                        .unwrap_or(stock_icon)
                } else if pointed {
                    rows.icon
                        .hover
                        .map(crate::themes::argb)
                        .unwrap_or(stock_icon)
                } else {
                    rows.icon
                        .colour
                        .map(crate::themes::argb)
                        .unwrap_or(stock_icon)
                }
            });
            face = face.push(self.cx.look.glyph(
                room.icon,
                icon_size.round() as u32,
                faded(icon_ink, opacity),
            ));
        }
        let marked = self.cx.themes.marked(
            &theme.id,
            yapfast_core::themes::chrome::Place::Channel,
            &room.name,
        );
        let name = crate::themes::list_case(list.and_then(|rows| rows.case), &marked);
        let row_role = list
            .and_then(|rows| rows.font)
            .map_or("body", yapfast_core::themes::lists::FontRole::name);
        let row_font = crate::themes::list_font(
            theme,
            list.and_then(|rows| rows.font),
            "body",
            list.and_then(|rows| rows.weight),
            MEDIUM,
        );
        let selected_shadow = if room.selected {
            crate::themes::offset_shadow(
                list.and_then(|rows| rows.ink.selected_shadow)
                    .map(|c| (c, 0.0, 1.0)),
            )
        } else {
            crate::piece::TextLight::default()
        };
        let name = elide(name.into_owned())
            .size(theme.type_size(row_role, list.and_then(|rows| rows.size).unwrap_or(15.0)))
            .font(row_font)
            .color(ink)
            .light(selected_shadow);
        let mut name = column![name].width(Length::Fill);
        if room.selected
            && let Some(underline) = list.and_then(|rows| rows.selected.underline)
        {
            name = name.spacing(underline.offset).push(bar(
                crate::themes::argb(underline.colour),
                Length::Fill,
                underline.width,
            ));
        }
        face = face.push(name);
        if let Some(started) = room.started {
            let lasted = yapfast_core::clock::lasted(started, self.cx.minute);
            face = face.push(
                text(lasted)
                    .size(theme.type_size("small", 12.0))
                    .font(theme.font("small", crate::fonts::REGULAR))
                    .color(theme.muted),
            );
        }
        if room.voice
            && let Some(meta) = list.map(|rows| &rows.meta)
            && !meta.voice.is_empty()
        {
            let count = room.people.len().to_string();
            let words = yapfast_core::themes::chrome::fill(&meta.voice, &[("n", count.as_str())]);
            let meta_role = meta
                .font
                .map_or("small", yapfast_core::themes::lists::FontRole::name);
            face = face.push(
                text(words)
                    .size(
                        meta.size
                            .unwrap_or_else(|| theme.type_size(meta_role, 11.0)),
                    )
                    .font(theme.font(meta_role, crate::fonts::REGULAR))
                    .color(meta.colour.map(crate::themes::argb).unwrap_or(theme.muted)),
            );
        }
        if !room.badge.is_empty() {
            let badge = list.map(|rows| rows.meta.badge).unwrap_or_default();
            face = face.push(themed_count_pill(theme, &room.badge, badge));
        } else if room.unread
            && list.and_then(|rows| rows.meta.unread)
                == Some(yapfast_core::themes::lists::UnreadMark::Badge)
        {
            let badge = list.map(|rows| rows.meta.badge).unwrap_or_default();
            face = face.push(themed_count_pill(theme, "•", badge));
        } else if room.unread
            && list.and_then(|rows| rows.meta.unread)
                == Some(yapfast_core::themes::lists::UnreadMark::Glyph)
            && let Some(glyph) = list.and_then(|rows| rows.meta.unread_glyph.as_ref())
        {
            let side = list.and_then(|rows| rows.meta.size).unwrap_or(11.0);
            match glyph {
                yapfast_core::themes::lists::Glyph::Text(glyph) => {
                    face = face.push(
                        text(glyph.clone()).size(side).color(
                            list.and_then(|rows| rows.meta.colour)
                                .map(crate::themes::argb)
                                .unwrap_or(theme.muted),
                        ),
                    );
                }
                yapfast_core::themes::lists::Glyph::Art(art) => {
                    if let Some(art) = crate::theme_art::tile_art(
                        self.cx,
                        yapfast_core::themes::package::art::ArtSurface::Sidebar,
                        art,
                        side,
                        side,
                    ) {
                        face = face.push(art);
                    }
                }
            }
        }
        // A voice channel's chat opens without joining it, from a button
        // shown under the pointer or while it's selected, as in Discord.
        if room.chat && (pointed || room.selected) {
            face = face.push(
                self.cx
                    .look
                    .icon_button(
                        Icon::Forum,
                        "Open chat",
                        Message::Side(Event::OpenChat(room.id.clone())),
                    )
                    .size(CHAT_BUTTON, 16),
            );
        }
        let message = if room.voice {
            Message::Side(Event::Voice(room.id.clone()))
        } else {
            Message::Side(Event::Select(room.id.clone()))
        };
        let row = self.row_face(
            room.id.clone(),
            face.into(),
            room.row_height(theme),
            room.selected,
            (!dragged).then_some(message),
            Some(room.id.clone()),
        );
        // A busy voice channel's card beside it, under the pointer.
        if pointed && room.voice && !room.people.is_empty() && !self.s.voice_card_off {
            return iced::widget::tooltip(
                row,
                self.voice_channel_card(&room.people),
                iced::widget::tooltip::Position::Right,
            )
            .gap(VOICE_CARD_GAP)
            .snap_within_viewport(true)
            .into();
        }
        row
    }

    /// A row's ink, and its opacity: a muted one is faint until it's
    /// selected or pointed at.
    pub(super) fn row_ink(self, room: &Room, pointed: bool) -> (Color, f32) {
        let theme = self.cx.look.theme;
        let stock = if room.locked {
            if room.selected {
                theme.muted
            } else {
                theme.faint
            }
        } else if room.selected {
            theme.token_colour("row-selected-text", theme.text)
        } else if room.unread {
            theme.text
        } else if pointed {
            theme.secondary
        } else {
            theme.muted
        };
        let ink = self
            .cx
            .themes
            .channel_rows(&theme.id)
            .and_then(|rows| {
                if room.muted && !room.selected && !pointed {
                    rows.ink.muted
                } else if room.selected {
                    rows.ink.selected
                } else if pointed {
                    rows.ink.hover
                } else if room.unread {
                    rows.ink.unread
                } else {
                    rows.ink.read
                }
            })
            .map(crate::themes::argb)
            .unwrap_or(stock);
        let opacity = if room.muted && !room.selected && !pointed {
            0.5
        } else {
            1.0
        };
        (faded(ink, opacity), opacity)
    }
}
