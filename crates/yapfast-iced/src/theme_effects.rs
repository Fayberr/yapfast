//! Owner-theme text light and pointer effects on ordinary iced content.
//!
//! These stay beside the app's resolved package registry: Wired's kit
//! [`Theme`](crate::theme::Theme) remains the one colour/style boundary and
//! keeps its original shape.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::{Duration, Instant},
};

use iced::{
    Color, Element, Font, Length, Point, Rectangle, Size, Transformation, Vector,
    advanced::{
        Layout, Renderer as _, Shell, Widget, layout, mouse, renderer,
        widget::{self, Tree},
    },
};
use yapfast_core::themes::{
    Performance, motion,
    package::{
        Animation, Curve, Property, Repeat, Slot,
        art::{Event, EventSlot, Foil, FoilSurface, TextRole},
    },
};

use crate::{
    app::Message,
    piece::TextLight,
    widgets::{At, Hooks, Inner, Wrap},
};

/// A resolved role's shadow/glow/outline/emboss in the renderer-neutral kit
/// form used by selectable rich text.
pub fn text_light(ctx: crate::ctx::Ctx<'_>, role: TextRole) -> TextLight {
    let Some(look) = ctx.themes.text_look(&ctx.look.theme.id, role) else {
        return TextLight::default();
    };
    let shadow = look.shadow.map(|shadow| {
        (
            crate::themes::argb(shadow.colour),
            Vector::new(shadow.x, shadow.y),
        )
    });
    let glow = look.glow.map(|(colour, reach)| {
        let mut colour = crate::themes::argb(colour);
        // The owner renderer draws two glow copies; one third alpha makes
        // their overlap build near the glyphs without flattening the face.
        colour.a /= 3.0;
        (colour, reach)
    });
    TextLight {
        shadow,
        glow,
        outline: look
            .outline
            .map(|(width, colour)| (crate::themes::argb(colour), width)),
        emboss: look.emboss,
    }
}

/// The owner's decrypt-on-hover event for a person name. The callback form
/// lets the generic kit widget animate core's canonical text algorithm
/// without knowing about package types.
pub fn name_hover_effect(
    ctx: crate::ctx::Ctx<'_>,
    words: &str,
) -> Option<crate::elide::HoverTextEffect> {
    if ctx.look.theme.is_readable_text() {
        return None;
    }
    let id = &ctx.look.theme.id;
    let event = ctx.themes.event_effect(id, EventSlot::NameHover)?;
    let (performance, reduced) = ctx.themes.art_motion(id);
    if !motion_runs(performance, reduced, ctx.plays) || !ctx.pace.runs() {
        return None;
    }
    let mut hash = DefaultHasher::new();
    words.hash(&mut hash);
    let base_fps = if performance == Performance::Full {
        60
    } else {
        30
    };
    let fps = ctx.pace.cap_fps(base_fps).max(1);
    Some(crate::elide::HoverTextEffect {
        duration_ms: event.duration,
        seed: hash.finish(),
        every: Duration::from_millis(1000 / fps),
        render: decrypt,
    })
}

fn decrypt(words: &str, elapsed: f32, duration: f32, seed: u64) -> (String, bool) {
    yapfast_core::themes::motion::effect_text(
        yapfast_core::themes::package::Effect::Decrypt,
        words,
        elapsed,
        duration,
        seed,
    )
}

/// A simple one-line label with the same owner text-light treatment as a
/// selectable [`crate::piece::Piece`]. Complex message text uses
/// [`text_light`] directly so its links and selection remain one paragraph.
pub fn label<'a>(
    ctx: crate::ctx::Ctx<'a>,
    role: TextRole,
    words: impl Into<String>,
    size: f32,
    font: Font,
    ink: Color,
) -> Element<'a, Message> {
    let light = text_light(ctx, role);
    crate::elide::elide(words.into())
        .size(size)
        .font(font)
        .color(ink)
        .light(light)
        .shrink()
        .into()
}

#[derive(Clone)]
struct Effects {
    click: Option<Event>,
    hover: Option<Event>,
    select: Option<(Event, bool)>,
    foil: Option<Foil>,
    running: bool,
    fps: u64,
}

#[derive(Default)]
struct State {
    hovered: bool,
    pointer: Point,
    hover_started: Option<Instant>,
    press_started: Option<Instant>,
    released: Option<Instant>,
    selected: bool,
    selected_started: Option<Duration>,
    elapsed: Duration,
    last: Option<Instant>,
    running: bool,
}

impl State {
    fn tick(&mut self, now: Instant, running: bool) {
        if running
            && self.running
            && let Some(last) = self.last
        {
            self.elapsed += now.saturating_duration_since(last);
        }
        self.last = Some(now);
        self.running = running;
    }

    fn animating(&self, effects: &Effects, now: Instant) -> bool {
        effects.foil.is_some()
            || effects.hover.is_some_and(|event| {
                self.hover_started
                    .is_some_and(|at| age(now, at) < event.duration / 1000.0)
            })
            || effects.click.is_some_and(|event| {
                self.press_started.is_some()
                    && (self.released.is_none()
                        || self
                            .released
                            .is_some_and(|at| age(now, at) < event.duration / 1000.0))
            })
    }
}

impl Hooks<Message> for Effects {
    type State = State;

    fn diff(&self, state: &mut State) {
        let selected = self.select.is_some_and(|(_, selected)| selected);
        if selected != state.selected {
            state.selected = selected;
            state.selected_started = selected.then_some(state.elapsed);
        }
    }

    fn update(
        &mut self,
        state: &mut State,
        event: &iced::Event,
        at: At,
        shell: &mut Shell<'_, Message>,
        child: &mut Inner<'_, '_, Message>,
    ) {
        let now = Instant::now();
        match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                let position = at.cursor.position();
                let hovered = position.is_some_and(|position| at.bounds.contains(position));
                if hovered && !state.hovered {
                    state.hover_started = Some(now);
                }
                state.hovered = hovered;
                if let Some(position) = position {
                    state.pointer = position;
                }
            }
            iced::Event::Mouse(iced::mouse::Event::CursorLeft) => state.hovered = false,
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
                if state.hovered && self.click.is_some() =>
            {
                state.press_started = Some(now);
                state.released = None;
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                if state.press_started.is_some() =>
            {
                state.released = Some(now);
            }
            iced::Event::Window(iced::window::Event::Unfocused) => {
                state.hovered = false;
                state.last = None;
                state.running = false;
            }
            iced::Event::Window(iced::window::Event::RedrawRequested(at)) => {
                state.tick(*at, self.running);
                if self.running && state.animating(self, *at) {
                    shell.request_redraw_at(*at + Duration::from_millis(1000 / self.fps));
                }
            }
            _ => {}
        }
        child.update(shell);
    }

    fn draw(
        &self,
        state: &State,
        renderer: &mut crate::widgets::Renderer,
        at: At,
        child: &mut dyn FnMut(&mut crate::widgets::Renderer),
    ) {
        use iced::advanced::Renderer as _;

        let bounds = at.bounds;
        renderer.with_layer(bounds, |renderer| {
            if let Some((event, selected)) = self.select
                && selected
            {
                let amount = if self.running {
                    state.last.map_or(1.0, |_now| {
                        state.selected_started.map_or(1.0, |started| {
                            (state.elapsed.saturating_sub(started).as_secs_f32() * 1000.0
                                / event.duration.max(1.0))
                            .clamp(0.0, 1.0)
                        })
                    })
                } else {
                    1.0
                };
                let mark = Rectangle::new(
                    Point::new(bounds.x + 2.0, bounds.y + bounds.height * 0.18),
                    iced::Size::new((bounds.width - 4.0).max(0.0) * amount, bounds.height * 0.64),
                );
                quad(
                    renderer,
                    mark,
                    crate::themes::argb(event.colour).scale_alpha(0.55),
                    2.0,
                );
            }

            child(renderer);

            let now = Instant::now();
            if self.running
                && let Some(event) = self.hover
                && state.hovered
                && let Some(started) = state.hover_started
            {
                let pass = (age(now, started) * 1000.0 / event.duration.max(1.0)).clamp(0.0, 1.0);
                if pass < 1.0 {
                    let width = bounds.width * 0.6;
                    let band = Rectangle::new(
                        Point::new(bounds.x - width + bounds.width * 1.6 * pass, bounds.y),
                        iced::Size::new(width, bounds.height),
                    );
                    band_gradient(renderer, band, crate::themes::argb(event.colour), 110.0);
                }
            }

            if self.running
                && let Some(event) = self.click
                && let Some(started) = state.press_started
            {
                let duration = (event.duration / 1000.0).max(0.001);
                let grow = (age(now, started) / duration).clamp(0.0, 1.0);
                let fade = state.released.map_or(1.0, |released| {
                    (1.0 - age(now, released) / duration).clamp(0.0, 1.0)
                });
                if fade > 0.0 {
                    let reach = bounds.width.max(bounds.height) * 2.2 * grow;
                    let center = if bounds.contains(state.pointer) {
                        state.pointer
                    } else {
                        bounds.center()
                    };
                    quad(
                        renderer,
                        Rectangle::new(
                            Point::new(center.x - reach / 2.0, center.y - reach / 2.0),
                            iced::Size::new(reach, reach),
                        ),
                        crate::themes::argb(event.colour).scale_alpha(fade),
                        reach / 2.0,
                    );
                }
            }

            if let Some(foil) = &self.foil {
                let position = if self.running {
                    if state.hovered && bounds.width > 0.0 {
                        ((state.pointer.x - bounds.x) / bounds.width).clamp(0.0, 1.0)
                    } else {
                        (state.elapsed.as_secs_f32() * 1000.0 / foil.period.max(1.0)).fract()
                    }
                } else {
                    0.5
                };
                let width = bounds.width * 0.9;
                let band = Rectangle::new(
                    Point::new(
                        bounds.x - bounds.width * 0.75 + bounds.width * 1.5 * position,
                        bounds.y,
                    ),
                    iced::Size::new(width, bounds.height),
                );
                let colours = foil.colours.map(crate::themes::argb);
                foil_gradient(renderer, band, colours, foil.opacity);
            }
        });
    }
}

/// Adds the owner's click ripple and hover gloss to an ordinary interactive
/// surface. Missing declarations return Wired's element unchanged.
pub fn interactive<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
) -> Element<'a, Message> {
    decorated(content, ctx, None, None)
}

/// Adds a selected-row highlighter as well as ordinary click/hover effects.
pub fn selected_row<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    selected: bool,
) -> Element<'a, Message> {
    decorated(
        content,
        ctx,
        Some((EventSlot::Select, selected, true)),
        None,
    )
}

/// Adds the owner's message-row highlighter while the pointer is on the row.
/// Unlike the selected-channel mark, this is deliberately absent in a still
/// theme, matching the owner renderer's reduced-motion behaviour.
pub fn message_row<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    hovered: bool,
) -> Element<'a, Message> {
    decorated(content, ctx, Some((EventSlot::Row, hovered, false)), None)
}

/// Whether the animated owner row mark replaces Wired's ordinary hover wash.
pub fn message_row_active(ctx: crate::ctx::Ctx<'_>) -> bool {
    let id = &ctx.look.theme.id;
    let (performance, reduced) = ctx.themes.art_motion(id);
    ctx.themes.event_effect(id, EventSlot::Row).is_some()
        && motion_runs(performance, reduced, ctx.plays)
        && ctx.pace.runs()
}

/// Adds the owner's holographic band to a card or tile, together with the
/// ordinary pointer effects declared for that surface.
pub fn foil<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    surface: FoilSurface,
) -> Element<'a, Message> {
    decorated(content, ctx, None, Some(surface))
}

fn decorated<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    highlighter: Option<(EventSlot, bool, bool)>,
    foil_surface: Option<FoilSurface>,
) -> Element<'a, Message> {
    let id = &ctx.look.theme.id;
    let click = ctx.themes.event_effect(id, EventSlot::Click);
    let hover = ctx.themes.event_effect(id, EventSlot::Hover);
    let (performance, reduced) = ctx.themes.art_motion(id);
    let running = motion_runs(performance, reduced, ctx.plays) && ctx.pace.runs();
    let fps = ctx
        .pace
        .cap_fps(if performance == Performance::Full {
            60
        } else {
            30
        })
        .max(1);
    let select = highlighter.and_then(|(slot, selected, static_when_still)| {
        highlighter_effect(
            ctx.themes.event_effect(id, slot),
            selected,
            running,
            static_when_still,
        )
    });
    let foil = foil_surface.and_then(|surface| {
        ctx.themes
            .foil(id)
            .filter(|foil| foil.surfaces.contains(&surface))
            .cloned()
    });
    if click.is_none() && hover.is_none() && select.is_none() && foil.is_none() {
        return content.into();
    }
    Wrap::new(
        content,
        Effects {
            click,
            hover,
            select,
            foil,
            running,
            fps,
        },
    )
    .into()
}

fn age(now: Instant, since: Instant) -> f32 {
    now.saturating_duration_since(since).as_secs_f32()
}

fn motion_runs(performance: Performance, reduced: bool, plays: bool) -> bool {
    !reduced && performance != Performance::Light && (performance == Performance::Full || plays)
}

fn highlighter_effect(
    event: Option<Event>,
    selected: bool,
    running: bool,
    static_when_still: bool,
) -> Option<(Event, bool)> {
    (running || static_when_still)
        .then_some(event)
        .flatten()
        .map(|event| (event, selected))
}

fn quad(renderer: &mut crate::widgets::Renderer, bounds: Rectangle, colour: Color, radius: f32) {
    use iced::advanced::Renderer as _;
    renderer.fill_quad(
        iced::advanced::renderer::Quad {
            bounds,
            border: iced::Border::default().rounded(radius),
            ..Default::default()
        },
        colour,
    );
}

fn band_gradient(
    renderer: &mut crate::widgets::Renderer,
    bounds: Rectangle,
    colour: Color,
    angle: f32,
) {
    let clear = Color { a: 0.0, ..colour };
    crate::widgets::gradient_quad(
        renderer,
        bounds,
        0.0,
        angle,
        &[(0.0, clear), (0.5, colour), (1.0, clear)],
    );
}

fn foil_gradient(
    renderer: &mut crate::widgets::Renderer,
    bounds: Rectangle,
    colours: [Color; 3],
    opacity: f32,
) {
    let [a, b, c] = colours.map(|colour| colour.scale_alpha(opacity));
    crate::widgets::gradient_quad(
        renderer,
        bounds,
        0.0,
        90.0,
        &[
            (0.0, Color { a: 0.0, ..a }),
            (0.25, a),
            (0.5, b),
            (0.75, c),
            (1.0, Color { a: 0.0, ..c }),
        ],
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SlotPose {
    opacity: f32,
    x: f32,
    y: f32,
    scale: f32,
}

impl Default for SlotPose {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            x: 0.0,
            y: 0.0,
            scale: 1.0,
        }
    }
}

fn slot_pose(
    animations: &[Animation],
    slot: Slot,
    place: usize,
    elapsed_ms: f32,
    performance: Performance,
    reduced: bool,
) -> (SlotPose, bool, u64) {
    let mut pose = SlotPose::default();
    let mut any_moving = false;
    let mut fps = 0u32;
    for animation in animations {
        if animation.slot != slot || !motion::runs(animation, performance, reduced) {
            continue;
        }
        if animation.property == Property::Rotation || animation.property.is_colour() {
            continue;
        }
        let at = if slot == Slot::RailIcon {
            elapsed_ms - animation.phase * place as f32
        } else {
            elapsed_ms
        };
        if let motion::Value::Number(number) = motion::value_at(animation, at) {
            match animation.property {
                Property::Opacity => pose.opacity *= number,
                Property::X => pose.x += number,
                Property::Y => pose.y += number,
                Property::Scale => pose.scale *= number,
                Property::Tint | Property::Colour | Property::Rotation => {}
            }
        }
        if !motion::finished(animation, at) {
            any_moving = true;
            fps = fps.max(motion::frame_rate(animation, performance));
        }
    }
    (pose, any_moving, u64::from(fps.max(1)))
}

struct SlotWrap {
    animations: Vec<Animation>,
    slot: Slot,
    place: usize,
    ground: Option<Color>,
    performance: Performance,
    reduced: bool,
    running: bool,
    fps: u64,
    moving: bool,
}

#[derive(Default)]
struct SlotState {
    elapsed: Duration,
    last: Option<Instant>,
    running: bool,
}

impl SlotState {
    fn tick(&mut self, now: Instant, running: bool) {
        if running
            && self.running
            && let Some(last) = self.last
        {
            self.elapsed += now.saturating_duration_since(last);
        }
        self.last = Some(now);
        self.running = running;
    }
}

impl Hooks<Message> for SlotWrap {
    type State = SlotState;

    fn update(
        &mut self,
        state: &mut SlotState,
        event: &iced::Event,
        _at: At,
        shell: &mut Shell<'_, Message>,
        child: &mut Inner<'_, '_, Message>,
    ) {
        match event {
            iced::Event::Window(iced::window::Event::Unfocused) => {
                state.last = None;
                state.running = false;
            }
            iced::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                let active = self.running && self.moving;
                state.tick(*now, active);
                if active {
                    shell.request_redraw_at(*now + Duration::from_millis(1000 / self.fps));
                }
            }
            _ => {}
        }
        child.update(shell);
    }

    fn draw(
        &self,
        state: &SlotState,
        renderer: &mut crate::widgets::Renderer,
        at: At,
        child: &mut dyn FnMut(&mut crate::widgets::Renderer),
    ) {
        let elapsed_ms = state.elapsed.as_secs_f32() * 1000.0;
        let (pose, _, _) = slot_pose(
            &self.animations,
            self.slot,
            self.place,
            elapsed_ms,
            self.performance,
            self.reduced,
        );
        let opacity = pose.opacity.clamp(0.0, 1.0);
        if opacity <= 0.005 {
            return;
        }
        let transform =
            (pose.x != 0.0 || pose.y != 0.0 || (pose.scale - 1.0).abs() > 0.001).then(|| {
                let c = at.child.center();
                let s = pose.scale.max(0.01);
                Transformation::translate(pose.x + c.x, pose.y + c.y)
                    * Transformation::scale(s)
                    * Transformation::translate(-c.x, -c.y)
            });
        let mut draw_child = |renderer: &mut crate::widgets::Renderer| {
            if let Some(t) = transform {
                renderer.with_transformation(t, |renderer| child(renderer));
            } else {
                child(renderer);
            }
            if opacity < 0.995
                && let Some(ground) = self.ground
            {
                quad(renderer, at.bounds, ground.scale_alpha(1.0 - opacity), 0.0);
            }
        };
        draw_child(renderer);
    }
}

/// Applies non-rotating declared animations (`Opacity`, `X`, `Y`, `Scale`)
/// for `slot`, leaving Wired's layout and hit target unchanged.
pub fn slot<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    slot: Slot,
    ground: Option<Color>,
) -> Element<'a, Message> {
    slot_at(content, ctx, slot, 0, ground)
}

/// Applies non-rotating declared animations for a rail tile at `place`, using
/// `phase * place` timing for `Slot::RailIcon`.
pub fn rail_icon<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    home: bool,
    place: usize,
) -> Element<'a, Message> {
    let slot = if home { Slot::Home } else { Slot::RailIcon };
    slot_at(content, ctx, slot, place, Some(ctx.look.theme.rail))
}

fn slot_at<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    slot: Slot,
    place: usize,
    ground: Option<Color>,
) -> Element<'a, Message> {
    let id = &ctx.look.theme.id;
    let animations: Vec<_> = ctx
        .themes
        .animations(id)
        .iter()
        .filter(|a| a.slot == slot && a.property != Property::Rotation && !a.property.is_colour())
        .cloned()
        .collect();
    if animations.is_empty() {
        return content.into();
    }
    let (performance, reduced) = ctx.themes.art_motion(id);
    if !animations
        .iter()
        .any(|a| motion::runs(a, performance, reduced))
    {
        return content.into();
    }
    let running = motion_runs(performance, reduced, ctx.plays) && ctx.pace.runs();
    let (_, moving, base_fps) = slot_pose(&animations, slot, place, 0.0, performance, reduced);
    let moving = moving || animations.iter().any(|a| a.repeat != Repeat::Once);
    let fps = ctx.pace.cap_fps(base_fps).max(1);
    Wrap::new(
        content,
        SlotWrap {
            animations,
            slot,
            place,
            ground,
            performance,
            reduced,
            running,
            fps,
            moving,
        },
    )
    .into()
}

fn over_argb(under: u32, over: u32) -> u32 {
    let oa = ((over >> 24) & 0xff) as f32 / 255.0;
    let ua = ((under >> 24) & 0xff) as f32 / 255.0;
    let out_a = oa + ua * (1.0 - oa);
    if out_a <= 0.0001 {
        return 0;
    }
    let mix_chan = |shift: u32| -> u32 {
        let oc = ((over >> shift) & 0xff) as f32;
        let uc = ((under >> shift) & 0xff) as f32;
        (((oc * oa + uc * ua * (1.0 - oa)) / out_a).round() as u32).min(255)
    };
    let a = (out_a * 255.0).round().clamp(0.0, 255.0) as u32;
    (a << 24) | (mix_chan(16) << 16) | (mix_chan(8) << 8) | mix_chan(0)
}

fn window_tint_colour(
    animations: &[Animation],
    elapsed_ms: f32,
    chat_ground: Color,
    performance: Performance,
    reduced: bool,
) -> (Option<Color>, bool, u64) {
    let mut tint: Option<u32> = None;
    let mut moving = false;
    let mut fps = 0u32;
    let ground_rgb = (((chat_ground.r * 255.0).round() as u32 & 0xff) << 16)
        | (((chat_ground.g * 255.0).round() as u32 & 0xff) << 8)
        | ((chat_ground.b * 255.0).round() as u32 & 0xff);
    for animation in animations {
        if animation.slot != Slot::Window || !motion::runs(animation, performance, reduced) {
            continue;
        }
        match (animation.property, motion::value_at(animation, elapsed_ms)) {
            (Property::Tint, motion::Value::Colour(colour)) => {
                tint = Some(tint.map_or(colour, |under| over_argb(under, colour)));
            }
            (Property::Opacity, motion::Value::Number(opacity)) => {
                let alpha = ((1.0 - opacity).clamp(0.0, 1.0) * 255.0).round() as u32;
                let colour = (alpha << 24) | ground_rgb;
                tint = Some(tint.map_or(colour, |under| over_argb(under, colour)));
            }
            _ => continue,
        }
        if animation.repeat != Repeat::Once || !motion::finished(animation, elapsed_ms) {
            moving = true;
            fps = fps.max(motion::frame_rate(animation, performance));
        }
    }
    (
        tint.filter(|c| (c >> 24) > 0).map(crate::themes::argb),
        moving,
        u64::from(fps.max(1)),
    )
}

struct WindowTint {
    animations: Vec<Animation>,
    chat_ground: Color,
    performance: Performance,
    reduced: bool,
    running: bool,
    fps: u64,
    moving: bool,
}

impl<M> Widget<M, iced::Theme, crate::widgets::Renderer> for WindowTint {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<SlotState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(SlotState::default())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &crate::widgets::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &crate::widgets::Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<SlotState>();
        match event {
            iced::Event::Window(iced::window::Event::Unfocused) => {
                state.last = None;
                state.running = false;
            }
            iced::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                let active = self.running && self.moving;
                state.tick(*now, active);
                if active {
                    shell.request_redraw_at(*now + Duration::from_millis(1000 / self.fps));
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut crate::widgets::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<SlotState>();
        let (colour, _, _) = window_tint_colour(
            &self.animations,
            state.elapsed.as_secs_f32() * 1000.0,
            self.chat_ground,
            self.performance,
            self.reduced,
        );
        if let Some(colour) = colour {
            quad(renderer, layout.bounds(), colour, 0.0);
        }
    }
}

/// Overlay for declared `Slot::Window` (`Tint` / `Opacity`) animations.
pub fn window_tint<'a>(
    content: Element<'a, Message>,
    ctx: crate::ctx::Ctx<'a>,
) -> Element<'a, Message> {
    let id = &ctx.look.theme.id;
    let animations: Vec<_> = ctx
        .themes
        .animations(id)
        .iter()
        .filter(|a| a.slot == Slot::Window)
        .cloned()
        .collect();
    if animations.is_empty() {
        return content;
    }
    let (performance, reduced) = ctx.themes.art_motion(id);
    if !animations
        .iter()
        .any(|a| motion::runs(a, performance, reduced))
    {
        return content;
    }
    let running = motion_runs(performance, reduced, ctx.plays) && ctx.pace.runs();
    let (_, moving, base_fps) =
        window_tint_colour(&animations, 0.0, ctx.look.theme.chat, performance, reduced);
    let fps = ctx.pace.cap_fps(base_fps).max(1);
    let tint: Element<'a, Message> = Element::new(WindowTint {
        animations,
        chat_ground: ctx.look.theme.chat,
        performance,
        reduced,
        running,
        fps,
        moving,
    });
    iced::widget::stack![content, tint]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Evaluates `Slot::Accent` at `elapsed_ms`, returning the animated accent
/// colour and recommended redraw interval when an accent animation runs.
pub fn accent_animation(
    themes: &crate::themes::Themes,
    id: &str,
    elapsed_ms: f32,
    pace: crate::themes::Pace,
    plays: bool,
) -> Option<(Color, Option<Duration>)> {
    let (performance, reduced) = themes.art_motion(id);
    let mut colour = None;
    let mut fps = 0u32;
    let mut moving = false;
    for animation in themes.animations(id) {
        if animation.slot != Slot::Accent
            || animation.property != Property::Colour
            || !motion::runs(animation, performance, reduced)
        {
            continue;
        }
        if let motion::Value::Colour(c) = motion::value_at(animation, elapsed_ms) {
            colour = Some(crate::themes::argb(c));
        }
        if !motion::finished(animation, elapsed_ms) {
            moving = true;
            fps = fps.max(motion::frame_rate(animation, performance));
        }
    }
    let colour = colour?;
    let every = (moving && motion_runs(performance, reduced, plays) && pace.runs()).then(|| {
        let capped = pace.cap_fps(u64::from(fps.max(1))).max(1);
        Duration::from_millis(1000 / capped)
    });
    Some((colour, every))
}

#[derive(Clone, Copy)]
struct ReactionStamp {
    mine: bool,
    duration_ms: f32,
    running: bool,
    fps: u64,
}

#[derive(Default)]
struct StampState {
    initialized: bool,
    was_mine: bool,
    started: Option<Instant>,
}

impl Hooks<Message> for ReactionStamp {
    type State = StampState;

    fn diff(&self, state: &mut StampState) {
        if !state.initialized {
            state.initialized = true;
            state.was_mine = self.mine;
            return;
        }
        if self.mine && !state.was_mine && self.running {
            state.started = Some(Instant::now());
        }
        state.was_mine = self.mine;
    }

    fn update(
        &mut self,
        state: &mut StampState,
        event: &iced::Event,
        _at: At,
        shell: &mut Shell<'_, Message>,
        child: &mut Inner<'_, '_, Message>,
    ) {
        if let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event
            && let Some(started) = state.started
        {
            let elapsed_ms = age(*now, started) * 1000.0;
            if elapsed_ms < self.duration_ms.max(1.0) && self.running {
                shell.request_redraw_at(*now + Duration::from_millis(1000 / self.fps));
            } else {
                state.started = None;
            }
        }
        child.update(shell);
    }

    fn draw(
        &self,
        state: &StampState,
        renderer: &mut crate::widgets::Renderer,
        at: At,
        child: &mut dyn FnMut(&mut crate::widgets::Renderer),
    ) {
        let Some(started) = state.started else {
            child(renderer);
            return;
        };
        let t = (age(Instant::now(), started) * 1000.0 / self.duration_ms.max(1.0)).clamp(0.0, 1.0);
        if t >= 1.0 {
            child(renderer);
            return;
        }
        let thunk = 1.0 - motion::ease(Curve::Bounce, t);
        let scale = 1.0 + 0.5 * thunk;
        let c = at.child.center();
        let transformation = Transformation::translate(c.x, c.y)
            * Transformation::scale(scale)
            * Transformation::translate(-c.x, -c.y);
        renderer.with_transformation(transformation, child);
    }
}

/// Reaction stamp bounce (`EventSlot::Reaction`, scale-only without deferred
/// rotation) when the user toggles a reaction on (`mine: false -> true`).
pub fn reaction_stamp<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    mine: bool,
) -> Element<'a, Message> {
    let id = &ctx.look.theme.id;
    let Some(event) = ctx.themes.event_effect(id, EventSlot::Reaction) else {
        return content.into();
    };
    let (performance, reduced) = ctx.themes.art_motion(id);
    let running = motion_runs(performance, reduced, ctx.plays) && ctx.pace.runs();
    if !running {
        return content.into();
    }
    let base_fps = if performance == Performance::Full {
        60
    } else {
        30
    };
    let fps = ctx.pace.cap_fps(base_fps).max(1);
    Wrap::new(
        content,
        ReactionStamp {
            mine,
            duration_ms: event.duration,
            running,
            fps,
        },
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foil_surface_selection_and_event_age_are_bounded() {
        assert_eq!(age(Instant::now(), Instant::now()), 0.0);
        let foil = Foil {
            surfaces: vec![FoilSurface::Card],
            colours: [0xffff_0000, 0xff00_ff00, 0xff00_00ff],
            opacity: 0.5,
            period: 4000.0,
        };
        assert!(foil.surfaces.contains(&FoilSurface::Card));
        assert!(!foil.surfaces.contains(&FoilSurface::Tile));
    }

    #[test]
    fn performance_and_reduce_motion_gate_effect_animation() {
        assert!(motion_runs(Performance::Full, false, false));
        assert!(motion_runs(Performance::Balanced, false, true));
        assert!(!motion_runs(Performance::Balanced, false, false));
        assert!(!motion_runs(Performance::Light, false, true));
        assert!(!motion_runs(Performance::Full, true, true));
    }

    #[test]
    fn only_the_selection_highlighter_survives_a_still_theme() {
        let event = Event {
            colour: 0xffff_e45c,
            duration: 260.0,
        };
        assert_eq!(
            highlighter_effect(Some(event), true, false, true),
            Some((event, true))
        );
        assert_eq!(highlighter_effect(Some(event), true, false, false), None);
        assert_eq!(
            highlighter_effect(Some(event), true, true, false),
            Some((event, true))
        );
    }

    #[test]
    fn non_rotating_slot_pose_window_tint_and_accent_colour_evaluate() {
        let slot_anims = vec![
            Animation {
                slot: Slot::RailIcon,
                property: Property::Y,
                values: vec![0.0, 10.0],
                colours: Vec::new(),
                period: 1000.0,
                easing: Curve::Linear,
                bezier: None,
                repeat: Repeat::Loop,
                delay: 0.0,
                phase: 100.0,
                steps: None,
                heavy: false,
            },
            Animation {
                slot: Slot::RailIcon,
                property: Property::Rotation,
                values: vec![0.0, 90.0],
                colours: Vec::new(),
                period: 1000.0,
                easing: Curve::Linear,
                bezier: None,
                repeat: Repeat::Loop,
                delay: 0.0,
                phase: 0.0,
                steps: None,
                heavy: false,
            },
        ];
        let (p0, moving0, _) = slot_pose(
            &slot_anims,
            Slot::RailIcon,
            0,
            500.0,
            Performance::Full,
            false,
        );
        let (p1, _, _) = slot_pose(
            &slot_anims,
            Slot::RailIcon,
            2,
            500.0,
            Performance::Full,
            false,
        );
        assert!(moving0);
        assert!((p0.y - 5.0).abs() < 0.05);
        assert!((p1.y - 3.0).abs() < 0.05);

        let win_anims = vec![Animation {
            slot: Slot::Window,
            property: Property::Opacity,
            values: vec![0.5],
            colours: Vec::new(),
            period: 1000.0,
            easing: Curve::Linear,
            bezier: None,
            repeat: Repeat::Once,
            delay: 0.0,
            phase: 0.0,
            steps: None,
            heavy: false,
        }];
        let (tint, _, _) =
            window_tint_colour(&win_anims, 0.0, Color::BLACK, Performance::Full, false);
        let tint = tint.expect("window tint veil");
        assert!((tint.a - 0.5).abs() < 0.02);
    }
}
