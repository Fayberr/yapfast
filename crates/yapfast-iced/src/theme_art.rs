//! Installed-theme art behind ordinary iced surfaces: fitted and tiled image
//! layers, sprite sheets and drift, fake glass, particles and nine-slice list
//! art. These widgets deliberately have no input handling, so they cannot
//! change a surface's behaviour.

use std::time::{Duration, Instant};

use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, renderer, text,
    widget::{self, Operation, Tree},
};
use iced::{Background, Color, Element, Length, Point, Rectangle, Size, Theme, Vector};
use yapfast_core::themes::Performance;
use yapfast_core::themes::layout::{Border as ThemeBorder, BorderPlace, Gel, Stroke};
use yapfast_core::themes::package::art::{
    ArtSurface, Depth, Emitter, Fit, Overlay, OverlayKind, Preset, Trigger,
};

use crate::{
    app::Message,
    themes::{ArtLayer, GlassPane, Pace, ParticleEvent, ThemeParticles, TileArtImage},
    widgets::gradient_quad,
};

struct Art<'a> {
    layers: &'a [ArtLayer],
    performance: Performance,
    reduced_motion: bool,
    running: bool,
    pace: Pace,
}

struct Procedural<'a> {
    overlays: &'a [Overlay],
    performance: Performance,
    reduced_motion: bool,
    running: bool,
    pace: Pace,
}

#[derive(Default)]
struct MotionClock {
    elapsed: Duration,
    last: Option<Instant>,
    running: bool,
}

impl MotionClock {
    fn redraw(&mut self, now: Instant, running: bool) {
        if running
            && self.running
            && let Some(last) = self.last
        {
            self.elapsed += now.saturating_duration_since(last);
        }
        self.last = Some(now);
        self.running = running;
    }

    fn seconds(&self) -> f32 {
        self.elapsed.as_secs_f32()
    }
}

fn clock_tag() -> widget::tree::Tag {
    widget::tree::Tag::of::<MotionClock>()
}

fn clock_state() -> widget::tree::State {
    widget::tree::State::new(MotionClock::default())
}

fn tick_clock(tree: &mut Tree, event: &iced::Event, running: bool) {
    if let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event {
        tree.state
            .downcast_mut::<MotionClock>()
            .redraw(*now, running);
    }
}

fn clock_seconds(tree: &Tree) -> f32 {
    tree.state.downcast_ref::<MotionClock>().seconds()
}

fn animation_running(performance: Performance, plays: bool) -> bool {
    performance == Performance::Full || (performance == Performance::Balanced && plays)
}

fn frame_fps(performance: Performance, pace: Pace) -> u64 {
    let base = if performance == Performance::Full {
        60
    } else {
        30
    };
    pace.cap_fps(base).max(1)
}

#[derive(Clone, Copy)]
struct DashedOutline {
    border: ThemeBorder,
    width: f32,
    colour: Color,
    radius: f32,
}

struct ThemeRule {
    border: Option<ThemeBorder>,
    width: f32,
    colour: Color,
}

/// A package's IRC separator repeated through every wrapped line. It wraps
/// the message body so its height comes from the content it sits beside,
/// without measuring hundreds of separator lines on every layout pass.
struct IrcRule<'a> {
    separator: String,
    repeated: String,
    size: f32,
    line_height: f32,
    font: iced::Font,
    colour: Color,
    content: Element<'a, Message>,
}

impl Widget<Message, Theme, iced::Renderer> for IrcRule<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        use iced::advanced::text::Paragraph as _;

        const GAP: f32 = 6.0;

        let paragraph = <iced::Renderer as text::Renderer>::Paragraph::with_text(text::Text {
            content: self.separator.as_str(),
            bounds: Size::new(f32::INFINITY, self.line_height),
            size: self.size.into(),
            line_height: text::LineHeight::Absolute(self.line_height.into()),
            font: self.font,
            align_x: text::Alignment::Default,
            align_y: iced::alignment::Vertical::Center,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        });
        let rule_width = paragraph.min_bounds().width;
        let content = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &limits.shrink(Size::new(rule_width + GAP, 0.0)),
        );
        let content_bounds = content.bounds();
        layout::Node::with_children(
            Size::new(
                rule_width + GAP + content_bounds.width,
                content_bounds.height,
            ),
            vec![
                layout::Node::new(Size::new(rule_width, content_bounds.height)),
                content.move_to(Point::new(rule_width + GAP, 0.0)),
            ],
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout
                .children()
                .nth(1)
                .expect("the IRC rule's message body"),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout
                .children()
                .nth(1)
                .expect("the IRC rule's message body"),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout
                .children()
                .nth(1)
                .expect("the IRC rule's message body"),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::text::Renderer as _;

        let rule = layout
            .children()
            .next()
            .expect("the IRC rule's separator")
            .bounds();
        // As in the owner UI, three copies a third of a glyph apart make a
        // line-character separator join cleanly between wrapped lines.
        for shift in 0..3 {
            renderer.fill_text(
                text::Text {
                    content: self.repeated.clone(),
                    bounds: rule.size(),
                    size: self.size.into(),
                    line_height: text::LineHeight::Absolute(self.line_height.into()),
                    font: self.font,
                    align_x: text::Alignment::Default,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                },
                Point::new(rule.x, rule.y + self.size * shift as f32 / 3.0),
                self.colour,
                rule,
            );
        }

        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout
                .children()
                .nth(1)
                .expect("the IRC rule's message body"),
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout
                .children()
                .nth(1)
                .expect("the IRC rule's message body"),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<IrcRule<'a>> for Element<'a, Message> {
    fn from(rule: IrcRule<'a>) -> Self {
        Element::new(rule)
    }
}

/// The owner theme's crisp gel drawn at the resolved size of one bubble.
/// The child retains all of its own layout, input and overlay behaviour.
struct BubbleGel<'a> {
    content: Element<'a, Message>,
    background: Background,
    radius: f32,
    gel: Gel,
}

impl Widget<Message, Theme, iced::Renderer> for BubbleGel<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let shadow = self
            .gel
            .shadow
            .map_or_else(iced::Shadow::default, |shadow| iced::Shadow {
                color: crate::themes::argb(shadow.colour),
                offset: Vector::new(shadow.x, shadow.y),
                blur_radius: shadow.blur,
            });
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border::default().rounded(self.radius),
                shadow,
                ..Default::default()
            },
            self.background,
        );

        renderer.with_layer(bounds, |renderer| {
            if let Some(glow) = self.gel.glow {
                let height = glow.height.min(bounds.height).max(0.0);
                let glow_bounds = Rectangle::new(
                    Point::new(bounds.x, bounds.y + bounds.height - height),
                    Size::new(bounds.width, height),
                );
                gradient_quad(
                    renderer,
                    glow_bounds,
                    self.radius.min(height / 2.0),
                    180.0,
                    &[
                        (0.0, Color::TRANSPARENT),
                        (1.0, crate::themes::argb(glow.colour)),
                    ],
                );
            }
            if let Some(gloss) = self.gel.gloss {
                let height = (bounds.height * gloss.height).min(gloss.max).max(0.0);
                let width = (bounds.width - gloss.inset.0 * 2.0).max(0.0);
                if width > 0.0 && height > 0.0 {
                    let colour = crate::themes::argb(gloss.colour);
                    let at = |share: f32| Color {
                        a: colour.a * share.clamp(0.0, 1.0),
                        ..colour
                    };
                    let radius = gloss
                        .radius
                        .unwrap_or((self.radius - gloss.inset.0).max(0.0))
                        .min(width.min(height) / 2.0);
                    gradient_quad(
                        renderer,
                        Rectangle::new(
                            Point::new(bounds.x + gloss.inset.0, bounds.y + gloss.inset.1),
                            Size::new(width, height),
                        ),
                        radius,
                        180.0,
                        &[(0.0, at(gloss.from)), (1.0, at(gloss.to))],
                    );
                }
            }
        });

        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );

        if let Some(light) = self.gel.rim_light {
            let light = crate::themes::argb(light);
            let faded = |alpha: f32| Color {
                a: light.a * alpha,
                ..light
            };
            let inset = Rectangle::new(
                Point::new(bounds.x + 1.0, bounds.y + 1.0),
                Size::new(
                    (bounds.width - 2.0).max(0.0),
                    (bounds.height - 2.0).max(0.0),
                ),
            );
            if inset.width > 0.0 && inset.height > 0.0 {
                quad(
                    renderer,
                    Rectangle::new(inset.position(), Size::new(inset.width, 1.0)),
                    light,
                    0.0,
                );
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(inset.x, inset.y + inset.height - 1.0),
                        Size::new(inset.width, 1.0),
                    ),
                    faded(0.65),
                    0.0,
                );
                for x in [inset.x, inset.x + inset.width - 1.0] {
                    gradient_quad(
                        renderer,
                        Rectangle::new(Point::new(x, inset.y), Size::new(1.0, inset.height)),
                        0.0,
                        180.0,
                        &[
                            (0.0, light),
                            (0.4, faded(0.3)),
                            (0.7, faded(0.15)),
                            (1.0, faded(0.65)),
                        ],
                    );
                }
            }
        }
        if let Some(dark) = self.gel.rim_dark {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        color: crate::themes::argb(dark),
                        width: 1.0,
                        radius: self.radius.into(),
                    },
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<BubbleGel<'a>> for Element<'a, Message> {
    fn from(bubble: BubbleGel<'a>) -> Self {
        Element::new(bubble)
    }
}

impl<M> Widget<M, Theme, iced::Renderer> for ThemeRule {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.width))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(limits.max().width, self.width))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        match self.border {
            Some(border) if border.stroke != Stroke::Solid => {
                let dotted = border.stroke == Stroke::Dotted;
                let dash = if dotted {
                    self.width
                } else {
                    border.dash.max(self.width)
                };
                draw_dashes(
                    renderer,
                    bounds.position(),
                    bounds.width,
                    self.width,
                    dash,
                    border.gap,
                    true,
                    dotted,
                    self.colour,
                );
            }
            _ => quad(renderer, bounds, self.colour, 0.0),
        }
    }
}

impl<'a, M: 'a> From<ThemeRule> for Element<'a, M> {
    fn from(rule: ThemeRule) -> Self {
        Element::new(rule)
    }
}

impl<M> Widget<M, Theme, iced::Renderer> for DashedOutline {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let width = self.width.max(1.0);
        if self.border.stroke == Stroke::Solid {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        color: self.colour,
                        width,
                        radius: self.radius.into(),
                    },
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
            return;
        }
        let dotted = self.border.stroke == Stroke::Dotted;
        let dash = if dotted {
            width
        } else {
            self.border.dash.max(width)
        };
        let gap = self.border.gap.max(0.0);
        let inset = width / 2.0;
        let horizontal = (bounds.width - width).max(0.0);
        let vertical = (bounds.height - width).max(0.0);
        draw_dashes(
            renderer,
            Point::new(bounds.x + inset, bounds.y),
            horizontal,
            width,
            dash,
            gap,
            true,
            dotted,
            self.colour,
        );
        draw_dashes(
            renderer,
            Point::new(bounds.x + inset, bounds.y + bounds.height - width),
            horizontal,
            width,
            dash,
            gap,
            true,
            dotted,
            self.colour,
        );
        draw_dashes(
            renderer,
            Point::new(bounds.x, bounds.y + inset),
            vertical,
            width,
            dash,
            gap,
            false,
            dotted,
            self.colour,
        );
        draw_dashes(
            renderer,
            Point::new(bounds.x + bounds.width - width, bounds.y + inset),
            vertical,
            width,
            dash,
            gap,
            false,
            dotted,
            self.colour,
        );
    }
}

impl<'a, M: 'a> From<DashedOutline> for Element<'a, M> {
    fn from(outline: DashedOutline) -> Self {
        Element::new(outline)
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_dashes(
    renderer: &mut iced::Renderer,
    start: Point,
    length: f32,
    width: f32,
    dash: f32,
    gap: f32,
    horizontal: bool,
    dotted: bool,
    colour: Color,
) {
    let mut at = 0.0;
    while at < length {
        let run = dash.min(length - at);
        let origin = if horizontal {
            Point::new(start.x + at, start.y)
        } else {
            Point::new(start.x, start.y + at)
        };
        let size = if horizontal {
            Size::new(run, width)
        } else {
            Size::new(width, run)
        };
        quad(
            renderer,
            Rectangle::new(origin, size),
            colour,
            if dotted { width / 2.0 } else { 0.0 },
        );
        at += dash + gap;
    }
}

/// Adds an owner theme's dashed or dotted outline without changing the kit
/// Theme boundary. Missing border declarations return the original content.
pub fn outlined<'a>(
    content: impl Into<Element<'a, Message>>,
    cx: crate::ctx::Ctx<'a>,
    place: BorderPlace,
    surface: &str,
    stock_radius: f32,
    stock_width: f32,
    stock_colour: Color,
) -> Element<'a, Message> {
    let content = content.into();
    let Some(border) = cx
        .themes
        .package_layout(&cx.look.theme.id)
        .and_then(|layout| layout.borders.get(&place))
        .copied()
    else {
        return content;
    };
    let fallback = cx
        .look
        .theme
        .surface_border(surface, stock_radius, stock_width, stock_colour);
    let width = border.width.unwrap_or(fallback.width).max(1.0);
    let colour = border
        .colour
        .map(crate::themes::argb)
        .unwrap_or(fallback.color);
    iced::widget::stack![
        content,
        DashedOutline {
            border,
            width,
            colour,
            radius: cx.look.theme.surface_radius(surface, stock_radius),
        }
    ]
    .into()
}

/// A horizontal rule whose dash pattern comes from a layout border place.
pub fn rule<'a>(
    cx: crate::ctx::Ctx<'a>,
    place: BorderPlace,
    stock_colour: Color,
    stock_width: f32,
) -> Element<'a, Message> {
    let border = cx
        .themes
        .package_layout(&cx.look.theme.id)
        .and_then(|layout| layout.borders.get(&place))
        .copied();
    let width = border
        .and_then(|border| border.width)
        .unwrap_or(stock_width)
        .max(1.0);
    let colour = border
        .and_then(|border| border.colour)
        .map(crate::themes::argb)
        .unwrap_or(stock_colour);
    ThemeRule {
        border,
        width,
        colour,
    }
    .into()
}

/// The configured separator down every visual IRC line. A false `rule`
/// continues to use the ordinary one-off text widget in the message row.
pub fn irc_rule<'a>(
    separator: String,
    size: f32,
    line_height: f32,
    font: iced::Font,
    colour: Color,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    const RULE_LINES: usize = 240;
    let repeated = vec![separator.as_str(); RULE_LINES].join("\n");
    IrcRule {
        separator,
        repeated,
        size,
        line_height,
        font,
        colour,
        content,
    }
    .into()
}

/// Draws a non-empty owner gel around one bubble without changing the kit
/// theme boundary. Callers retain the stock container when the gel is empty.
pub fn gel_bubble<'a>(
    content: impl Into<Element<'a, Message>>,
    background: Background,
    radius: f32,
    gel: Gel,
) -> Element<'a, Message> {
    BubbleGel {
        content: content.into(),
        background,
        radius,
        gel,
    }
    .into()
}

impl<M> Widget<M, Theme, iced::Renderer> for Procedural<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> widget::tree::Tag {
        clock_tag()
    }

    fn state(&self) -> widget::tree::State {
        clock_state()
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
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
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let moving = self.running
            && !self.reduced_motion
            && self.performance != Performance::Light
            && self.overlays.iter().any(|overlay| {
                overlay.moves() && (self.performance == Performance::Full || !overlay.heavy)
            });
        tick_clock(tree, event, moving);
        if moving && let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event {
            let fps = frame_fps(self.performance, self.pace);
            shell.request_redraw_at(*now + Duration::from_millis(1000 / fps));
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let elapsed = clock_seconds(tree);
        renderer.with_layer(bounds, |renderer| {
            for overlay in self.overlays {
                if overlay.heavy && self.performance == Performance::Light {
                    continue;
                }
                let runs = self.running
                    && !self.reduced_motion
                    && self.performance != Performance::Light
                    && (self.performance == Performance::Full || !overlay.heavy);
                draw_overlay(renderer, overlay, bounds, elapsed, runs);
            }
        });
    }
}

impl<'a, M: 'a> From<Procedural<'a>> for Element<'a, M> {
    fn from(overlays: Procedural<'a>) -> Self {
        Element::new(overlays)
    }
}

fn overlay_colour(overlay: &Overlay) -> Color {
    let mut colour = encoded_colour(overlay.colour);
    colour.a *= overlay.opacity;
    colour
}

fn quad(renderer: &mut iced::Renderer, bounds: Rectangle, colour: Color, radius: f32) {
    use iced::advanced::Renderer as _;
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: iced::Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        colour,
    );
}

fn draw_overlay(
    renderer: &mut iced::Renderer,
    overlay: &Overlay,
    bounds: Rectangle,
    elapsed: f32,
    runs: bool,
) {
    let colour = overlay_colour(overlay);
    match overlay.kind {
        OverlayKind::Image => {}
        OverlayKind::Scanlines => {
            let spacing = overlay.size.max(2.0);
            let mut y = bounds.y + spacing - 1.0;
            while y < bounds.y + bounds.height {
                quad(
                    renderer,
                    Rectangle::new(Point::new(bounds.x, y), Size::new(bounds.width, 1.0)),
                    colour,
                    0.0,
                );
                y += spacing;
            }
        }
        OverlayKind::Lcd => {
            let size = overlay.size.max(2.0);
            let stripe = size * 3.0;
            let tints = [
                Color::from_rgb(1.0, 0.16, 0.16),
                Color::from_rgb(0.16, 1.0, 0.16),
                Color::from_rgb(0.16, 0.16, 1.0),
            ];
            let mut x = bounds.x;
            let mut index = 0;
            while x < bounds.x + bounds.width {
                let mut tint = tints[index % 3];
                tint.a = colour.a / 5.0;
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(x, bounds.y),
                        Size::new((size - 1.0).max(1.0), bounds.height),
                    ),
                    tint,
                    0.0,
                );
                x += size;
                index += 1;
            }
            let mut y = bounds.y + size - 1.0;
            while y < bounds.y + bounds.height {
                quad(
                    renderer,
                    Rectangle::new(Point::new(bounds.x, y), Size::new(bounds.width, 1.0)),
                    colour,
                    0.0,
                );
                y += size;
            }
            let _ = stripe;
        }
        OverlayKind::Grain => {
            let phase = if runs && overlay.period > 0.0 {
                (elapsed * 1000.0 / overlay.period).floor() as u32
            } else {
                0
            };
            let block = 4.0;
            let across = (bounds.width / block).ceil() as u32;
            let down = (bounds.height / block).ceil() as u32;
            let samples = ((bounds.width * bounds.height / 180.0) as u32).clamp(64, 1600);
            for sample in 0..samples {
                let hash = sample
                    .wrapping_mul(1_103_515_245)
                    .wrapping_add(phase.wrapping_mul(12_345))
                    .rotate_left((sample + phase) & 15);
                let x = hash % across.max(1);
                let y = hash.rotate_left(11) % down.max(1);
                let amount = 0.2 + (hash & 0xff) as f32 / 255.0 * 0.8;
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(bounds.x + x as f32 * block, bounds.y + y as f32 * block),
                        Size::new(block, block),
                    ),
                    colour.scale_alpha(amount),
                    0.0,
                );
            }
        }
        OverlayKind::Vignette => {
            let reach = (bounds.width.min(bounds.height) * 0.22).max(1.0);
            for step in 0..16 {
                let at = step as f32 / 16.0;
                let width = reach / 16.0 + 0.5;
                let alpha = (1.0 - at) * 0.11;
                let inset = at * reach;
                for edge in [
                    Rectangle::new(
                        Point::new(bounds.x + inset, bounds.y + inset),
                        Size::new((bounds.width - inset * 2.0).max(0.0), width),
                    ),
                    Rectangle::new(
                        Point::new(bounds.x + inset, bounds.y + bounds.height - inset - width),
                        Size::new((bounds.width - inset * 2.0).max(0.0), width),
                    ),
                    Rectangle::new(
                        Point::new(bounds.x + inset, bounds.y + inset),
                        Size::new(width, (bounds.height - inset * 2.0).max(0.0)),
                    ),
                    Rectangle::new(
                        Point::new(bounds.x + bounds.width - inset - width, bounds.y + inset),
                        Size::new(width, (bounds.height - inset * 2.0).max(0.0)),
                    ),
                ] {
                    quad(renderer, edge, colour.scale_alpha(alpha), 0.0);
                }
            }
        }
        OverlayKind::Band => {
            let height = overlay.size.max(1.0);
            let progress = if runs && overlay.period > 0.0 {
                (elapsed * 1000.0 / overlay.period).fract()
            } else {
                0.5
            };
            let top = bounds.y - height + progress * (bounds.height + height);
            for step in 0..12 {
                let band = height / 12.0 + 0.5;
                let distance = ((step as f32 + 0.5) / 12.0 - 0.5).abs() * 2.0;
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(bounds.x, top + step as f32 * height / 12.0),
                        Size::new(bounds.width, band),
                    ),
                    colour.scale_alpha(1.0 - distance),
                    0.0,
                );
            }
        }
        OverlayKind::Flare => {
            let pulse = if runs && overlay.period > 0.0 {
                0.8 + 0.2 * (elapsed * std::f32::consts::TAU * 1000.0 / overlay.period).sin()
            } else {
                1.0
            };
            let diameter = overlay.size.max(1.0) * pulse;
            let center = Point::new(
                bounds.x + bounds.width * overlay.at.0,
                bounds.y + bounds.height * overlay.at.1,
            );
            for step in (1..=16).rev() {
                let scale = step as f32 / 16.0;
                let side = diameter * scale;
                quad(
                    renderer,
                    Rectangle::new(
                        Point::new(center.x - side / 2.0, center.y - side / 2.0),
                        Size::new(side, side),
                    ),
                    colour.scale_alpha((1.0 - scale) * 0.12 + 0.01),
                    side / 2.0,
                );
            }
        }
    }
}

impl<M> Widget<M, Theme, iced::Renderer> for Art<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> widget::tree::Tag {
        clock_tag()
    }

    fn state(&self) -> widget::tree::State {
        clock_state()
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
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
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let moving = self.running
            && !self.reduced_motion
            && self.performance != Performance::Light
            && self.layers.iter().any(|art| {
                art.layer.moves() && (self.performance == Performance::Full || !art.layer.heavy)
            });
        tick_clock(tree, event, moving);
        if moving && let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event {
            let fps = frame_fps(self.performance, self.pace);
            shell.request_redraw_at(*now + Duration::from_millis(1000 / fps));
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let elapsed = clock_seconds(tree);
        renderer.with_layer(bounds, |renderer| {
            for art in self.layers {
                let runs = self.running
                    && !self.reduced_motion
                    && self.performance != Performance::Light
                    && (self.performance == Performance::Full || !art.layer.heavy);
                let drift = if runs {
                    Vector::new(art.layer.drift.0 * elapsed, art.layer.drift.1 * elapsed)
                } else {
                    Vector::new(0.0, 0.0)
                };
                let source = Size::new(art.width.max(1) as f32, art.height.max(1) as f32);
                if art.layer.fit == Fit::Tile {
                    let tile = source;
                    let start = Point::new(
                        bounds.x - ((bounds.x - drift.x).rem_euclid(tile.width)),
                        bounds.y - ((bounds.y - drift.y).rem_euclid(tile.height)),
                    );
                    let across = (bounds.width / tile.width).ceil() as usize + 2;
                    let down = (bounds.height / tile.height).ceil() as usize + 2;
                    for y in 0..down.min(128) {
                        for x in 0..across.min(128) {
                            draw(
                                renderer,
                                art,
                                Rectangle::new(
                                    start
                                        + Vector::new(
                                            x as f32 * tile.width,
                                            y as f32 * tile.height,
                                        ),
                                    tile,
                                ),
                                elapsed,
                                runs,
                            );
                        }
                    }
                    continue;
                }
                let size = if let Some((width, height)) = art.layer.size {
                    Size::new(width, height)
                } else {
                    match art.layer.fit {
                        Fit::Stretch => bounds.size(),
                        Fit::Contain => {
                            source
                                * (bounds.width / source.width).min(bounds.height / source.height)
                        }
                        Fit::Cover => {
                            source
                                * (bounds.width / source.width).max(bounds.height / source.height)
                        }
                        Fit::Tile => unreachable!(),
                    }
                };
                let position = Point::new(
                    bounds.x + (bounds.width - size.width) * art.layer.position.0,
                    bounds.y + (bounds.height - size.height) * art.layer.position.1,
                ) + drift;
                draw(renderer, art, Rectangle::new(position, size), elapsed, runs);
            }
        });
    }
}

fn draw(
    renderer: &mut iced::Renderer,
    art: &ArtLayer,
    bounds: Rectangle,
    elapsed: f32,
    runs: bool,
) {
    let handle = if runs && !art.frames.is_empty() {
        let period = art
            .layer
            .frames
            .map_or(1000.0, |frames| frames.period)
            .max(1.0);
        let index = ((elapsed * 1000.0 / period * art.frames.len() as f32).floor() as usize)
            % art.frames.len();
        &art.frames[index]
    } else {
        &art.handle
    };
    crate::widgets::draw_picture(renderer, handle, bounds, [0.0; 4], art.layer.opacity);
}

struct Glass<'a> {
    pane: &'a GlassPane,
    surface: ArtSurface,
}

impl<M> Widget<M, Theme, iced::Renderer> for Glass<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::{
            Renderer as _,
            image::{self as raster, Renderer as _},
        };

        let bounds = layout.bounds();
        let inset = (self.pane.style.gap / 2.0).max(0.0);
        let pane = Rectangle::new(
            bounds.position() + Vector::new(inset, inset),
            Size::new(
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            ),
        );
        if pane.width <= 0.0 || pane.height <= 0.0 {
            return;
        }
        let whole = Rectangle::new(
            Point::ORIGIN,
            Size::new(self.pane.window.0 as f32, self.pane.window.1 as f32),
        );
        renderer.draw_image(
            raster::Image {
                handle: self.pane.handle.clone(),
                filter_method: raster::FilterMethod::Linear,
                rotation: iced::Radians(0.0),
                border_radius: Default::default(),
                opacity: 1.0,
                snap: true,
            },
            whole,
            pane,
        );
        let encoded = self.pane.ground(self.surface);
        let mut ground = encoded_colour(encoded);
        ground.a *= self.pane.style.opacity;
        let radius = self.pane.style.radius;
        let (rim, rim_width) = self.pane.style.rim.unwrap_or((0, 0.0));
        renderer.fill_quad(
            renderer::Quad {
                bounds: pane,
                border: iced::Border {
                    color: encoded_colour(rim),
                    width: rim_width,
                    radius: radius.into(),
                },
                ..Default::default()
            },
            ground,
        );
        if let Some(highlight) = self.pane.style.highlight {
            let colour = encoded_colour(highlight);
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(pane.position(), Size::new(pane.width, 1.0)),
                    ..Default::default()
                },
                colour,
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(pane.position(), Size::new(1.0, pane.height)),
                    ..Default::default()
                },
                colour,
            );
        }
        if let Some(inset) = self.pane.style.inset {
            let colour = encoded_colour(inset.colour);
            let reach = inset.blur.max(1.0).min(pane.width.min(pane.height) / 3.0);
            for (at, size) in [
                (pane.position(), Size::new(pane.width, reach)),
                (pane.position(), Size::new(reach, pane.height)),
                (
                    Point::new(pane.x, pane.y + pane.height - reach),
                    Size::new(pane.width, reach),
                ),
                (
                    Point::new(pane.x + pane.width - reach, pane.y),
                    Size::new(reach, pane.height),
                ),
            ] {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(at, size),
                        ..Default::default()
                    },
                    colour.scale_alpha(0.22),
                );
            }
        }
    }
}

fn encoded_colour(encoded: u32) -> iced::Color {
    let [alpha, red, green, blue] = encoded.to_be_bytes();
    iced::Color::from_rgba8(red, green, blue, f32::from(alpha) / 255.0)
}

impl<'a, M: 'a> From<Glass<'a>> for Element<'a, M> {
    fn from(glass: Glass<'a>) -> Self {
        Element::new(glass)
    }
}

struct Particles<'a> {
    sets: &'a [ThemeParticles],
    events: &'a std::sync::Arc<std::sync::Mutex<Vec<ParticleEvent>>>,
    performance: Performance,
    reduced_motion: bool,
    running: bool,
    pace: Pace,
    depth: Depth,
}

impl<M> Widget<M, Theme, iced::Renderer> for Particles<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> widget::tree::Tag {
        clock_tag()
    }

    fn state(&self) -> widget::tree::State {
        clock_state()
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
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
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        if self.reduced_motion || self.performance == Performance::Light {
            tick_clock(tree, event, false);
            return;
        }
        let runs = |set: &ThemeParticles| {
            set.spec.z == self.depth && (self.performance == Performance::Full || !set.spec.heavy)
        };
        let ambient = self
            .sets
            .iter()
            .any(|set| runs(set) && set.spec.on == Trigger::Ambient && set.spec.rate > 0.0);
        let now = Instant::now();
        let events = self.events.lock().is_ok_and(|mut events| {
            let longest = self
                .sets
                .iter()
                .map(|set| set.spec.life)
                .fold(0.0, f32::max)
                .max(1.0);
            events
                .retain(|event| now.duration_since(event.started).as_secs_f32() * 1000.0 < longest);
            !events.is_empty()
        });
        let moving = self.running && (ambient || events);
        tick_clock(tree, event, moving);
        if moving && let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event {
            let fps = frame_fps(self.performance, self.pace);
            shell.request_redraw_at(*now + Duration::from_millis(1000 / fps));
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        if self.reduced_motion || self.performance == Performance::Light {
            return;
        }
        let bounds = layout.bounds();
        let elapsed = clock_seconds(tree);
        let events = self
            .events
            .lock()
            .map_or_else(|_| Vec::new(), |events| events.clone());
        let mut shown = 0usize;
        for (set_index, set) in self.sets.iter().enumerate() {
            if set.spec.z != self.depth
                || (self.performance == Performance::Balanced && set.spec.heavy)
            {
                continue;
            }
            let life = (set.spec.life / 1000.0).max(0.01);
            if set.spec.on == Trigger::Ambient && set.spec.rate > 0.0 {
                let count = set.spec.count.min(160) as usize;
                let cycle = life.max(count as f32 / set.spec.rate.max(0.01));
                for index in 0..count {
                    let age = (elapsed - index as f32 / set.spec.rate).rem_euclid(cycle);
                    if age < life && shown < 160 {
                        draw_particle(
                            renderer,
                            set,
                            bounds,
                            age,
                            set_index as u64 * 997 + index as u64,
                            None,
                            Trigger::Ambient,
                        );
                        shown += 1;
                    }
                }
            }
            for (event_index, event) in events.iter().enumerate() {
                if event.trigger != set.spec.on {
                    continue;
                }
                let age = Instant::now().duration_since(event.started).as_secs_f32();
                if age >= life {
                    continue;
                }
                for index in 0..set.spec.count.min(160) as usize {
                    if shown >= 160 {
                        break;
                    }
                    draw_particle(
                        renderer,
                        set,
                        bounds,
                        age,
                        ((event_index as u64 + 1) << 32) ^ (set_index as u64 * 997 + index as u64),
                        event.origin,
                        event.trigger,
                    );
                    shown += 1;
                }
            }
        }
    }
}

fn draw_particle(
    renderer: &mut iced::Renderer,
    set: &ThemeParticles,
    bounds: Rectangle,
    age: f32,
    seed: u64,
    event_origin: Option<(f32, f32)>,
    trigger: Trigger,
) {
    use iced::advanced::Renderer as _;
    let life = (set.spec.life / 1000.0).max(0.01);
    let progress = (age / life).clamp(0.0, 1.0);
    let r = |salt| particle_random(seed ^ salt);
    let near = |x: f32, y: f32| (x + (r(7) - 0.5) * 16.0, y + (r(11) - 0.5) * 16.0);
    let (mut x, mut y) = match set.spec.from {
        Emitter::Composer => (
            bounds.x + bounds.width * (0.1 + 0.8 * r(1)),
            bounds.y + bounds.height - 45.0 + 20.0 * r(2),
        ),
        Emitter::SendButton => near(
            bounds.x + bounds.width - 32.0,
            bounds.y + bounds.height - 36.0,
        ),
        Emitter::ApplyButton => near(bounds.center_x(), bounds.center_y()),
        Emitter::Pointer => event_origin.unwrap_or((bounds.center_x(), bounds.center_y())),
        Emitter::Window => match set.spec.preset {
            Preset::Bubbles => (
                bounds.x + bounds.width * r(1),
                bounds.y + bounds.height + 10.0,
            ),
            Preset::Scraps => (bounds.x + bounds.width * r(1), bounds.y - 10.0),
            _ => (
                bounds.x + bounds.width * r(1),
                bounds.y + bounds.height * r(2),
            ),
        },
    };
    let burst = !matches!(trigger, Trigger::Ambient | Trigger::ThemeApplied);
    let angle = r(3) * std::f32::consts::TAU;
    let (mut vx, mut vy) = match set.spec.preset {
        Preset::Bubbles => ((r(4) - 0.5) * 30.0, -60.0 - 80.0 * r(5)),
        Preset::Sparkles if burst => (
            angle.cos() * (80.0 + 120.0 * r(4)),
            angle.sin() * (80.0 + 120.0 * r(5)),
        ),
        Preset::Sparkles => ((r(4) - 0.5) * 10.0, (r(5) - 0.5) * 10.0),
        Preset::Dust => ((r(4) - 0.5) * 16.0, (r(5) - 0.5) * 10.0 - 4.0),
        Preset::Scraps if burst => ((r(4) - 0.5) * 220.0, -160.0 - 120.0 * r(5)),
        Preset::Scraps => ((r(4) - 0.5) * 30.0, 50.0 + 60.0 * r(5)),
    };
    match set.spec.preset {
        Preset::Bubbles => x += (age * 3.0 + r(13) * 100.0).sin() * 20.0,
        Preset::Sparkles => {
            let damping = (-2.5 * age).exp();
            vx *= damping;
            vy *= damping;
        }
        Preset::Scraps => {
            vy = (vy + 130.0 * age).min(90.0);
            x += (age * 2.0 + r(13) * 100.0).sin() * 40.0;
        }
        Preset::Dust => {}
    }
    x += vx * age;
    y += vy * age;
    let size = set.spec.size * (0.6 + 0.8 * r(17));
    let mut opacity = (progress * 8.0).min(1.0) * ((1.0 - progress) / 0.4).min(1.0);
    if set.spec.preset == Preset::Sparkles {
        opacity *= 0.6 + 0.4 * (age * 18.0 + r(19) * 100.0).sin();
    }
    let encoded = if set.spec.colours.is_empty() {
        0xffff_ffff
    } else {
        set.spec.colours[(r(23) * set.spec.colours.len() as f32) as usize % set.spec.colours.len()]
    };
    let colour = encoded_colour(encoded).scale_alpha(opacity.clamp(0.0, 1.0));
    let at = Rectangle::new(
        Point::new(x - size / 2.0, y - size / 2.0),
        Size::new(size, size),
    );
    if let Some(sprite) = &set.sprite {
        crate::widgets::draw_picture(renderer, sprite, at, [0.0; 4], opacity);
        return;
    }
    match set.spec.preset {
        Preset::Bubbles => renderer.fill_quad(
            renderer::Quad {
                bounds: at,
                border: iced::Border {
                    color: colour,
                    width: (size / 8.0).clamp(1.0, 3.0),
                    radius: (size / 2.0).into(),
                },
                ..Default::default()
            },
            colour.scale_alpha(0.12),
        ),
        Preset::Sparkles => {
            let thick = (size / 5.0).max(1.0);
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(x - thick / 2.0, y - size / 2.0),
                        Size::new(thick, size),
                    ),
                    ..Default::default()
                },
                colour,
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(x - size / 2.0, y - thick / 2.0),
                        Size::new(size, thick),
                    ),
                    ..Default::default()
                },
                colour,
            );
        }
        Preset::Dust => renderer.fill_quad(
            renderer::Quad {
                bounds: at,
                border: iced::Border {
                    radius: (size / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            colour,
        ),
        Preset::Scraps => renderer.fill_quad(
            renderer::Quad {
                bounds: at,
                ..Default::default()
            },
            colour,
        ),
    }
}

fn particle_random(mut value: u64) -> f32 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value >> 40) as f32 / (1u64 << 24) as f32
}

impl<'a, M: 'a> From<Particles<'a>> for Element<'a, M> {
    fn from(particles: Particles<'a>) -> Self {
        Element::new(particles)
    }
}

struct NineSlice<'a> {
    image: &'a TileArtImage,
    width: f32,
    height: f32,
    opacity: f32,
    performance: Performance,
    reduced_motion: bool,
    running: bool,
    pace: Pace,
}

impl<M> Widget<M, Theme, iced::Renderer> for NineSlice<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.width), Length::Fixed(self.height))
    }

    fn tag(&self) -> widget::tree::Tag {
        clock_tag()
    }

    fn state(&self) -> widget::tree::State {
        clock_state()
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(
            limits,
            Length::Fixed(self.width),
            Length::Fixed(self.height),
        )
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let moving = self.running
            && !self.reduced_motion
            && self.performance != Performance::Light
            && !self.image.frames.is_empty()
            && (self.performance == Performance::Full || !self.image.art.layer.heavy);
        tick_clock(tree, event, moving);
        if moving && let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event {
            let fps = frame_fps(self.performance, self.pace);
            shell.request_redraw_at(*now + Duration::from_millis(1000 / fps));
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::{
            Renderer as _,
            image::{self as raster, Renderer as _},
        };
        let slot = layout.bounds();
        let size = self
            .image
            .art
            .size
            .map_or(slot.size(), |(width, height)| Size::new(width, height));
        let target = Rectangle::new(
            Point::new(
                slot.x
                    + (slot.width - size.width) * self.image.art.layer.position.0
                    + self.image.art.offset.0,
                slot.y
                    + (slot.height - size.height) * self.image.art.layer.position.1
                    + self.image.art.offset.1,
            ),
            size,
        );
        let filter_method = if self.image.art.pixelated {
            raster::FilterMethod::Nearest
        } else {
            raster::FilterMethod::Linear
        };
        let runs = self.running
            && !self.reduced_motion
            && self.performance != Performance::Light
            && (self.performance == Performance::Full || !self.image.art.layer.heavy);
        let (handle, pieces) = if runs && !self.image.frames.is_empty() {
            let period = self
                .image
                .art
                .layer
                .frames
                .map_or(1000.0, |frames| frames.period)
                .max(1.0);
            let index = ((clock_seconds(tree) * 1000.0 / period * self.image.frames.len() as f32)
                .floor() as usize)
                % self.image.frames.len();
            let (handle, pieces) = &self.image.frames[index];
            (handle, pieces)
        } else {
            (&self.image.handle, &self.image.pieces)
        };
        let draw = |renderer: &mut iced::Renderer,
                    handle: &iced::advanced::image::Handle,
                    bounds: Rectangle| {
            renderer.draw_image(
                raster::Image {
                    handle: handle.clone(),
                    filter_method,
                    rotation: iced::Radians(0.0),
                    border_radius: Default::default(),
                    opacity: self.image.art.layer.opacity * self.opacity,
                    snap: self.image.art.pixelated,
                },
                bounds,
                slot,
            );
        };
        let (Some(slice), Some(pieces)) = (self.image.art.slice, pieces) else {
            draw(renderer, handle, target);
            return;
        };
        let density = self.image.art.density.max(0.01);
        let mut left = slice[3] / density;
        let mut right = slice[1] / density;
        let mut top = slice[0] / density;
        let mut bottom = slice[2] / density;
        if left + right > target.width && left + right > 0.0 {
            let scale = target.width / (left + right);
            left *= scale;
            right *= scale;
        }
        if top + bottom > target.height && top + bottom > 0.0 {
            let scale = target.height / (top + bottom);
            top *= scale;
            bottom *= scale;
        }
        let xs = [
            target.x,
            target.x + left,
            target.x + target.width - right,
            target.x + target.width,
        ];
        let ys = [
            target.y,
            target.y + top,
            target.y + target.height - bottom,
            target.y + target.height,
        ];
        renderer.with_layer(slot, |renderer| {
            for row in 0..3 {
                for column in 0..3 {
                    let bounds = Rectangle::new(
                        Point::new(xs[column], ys[row]),
                        Size::new(
                            (xs[column + 1] - xs[column]).max(0.0),
                            (ys[row + 1] - ys[row]).max(0.0),
                        ),
                    );
                    if bounds.width > 0.0
                        && bounds.height > 0.0
                        && let Some(handle) = &pieces[row * 3 + column]
                    {
                        draw(renderer, handle, bounds);
                    }
                }
            }
        });
    }
}

impl<'a, M: 'a> From<NineSlice<'a>> for Element<'a, M> {
    fn from(slice: NineSlice<'a>) -> Self {
        Element::new(slice)
    }
}

pub fn picked_tile<'a>(
    choices: &'a [yapfast_core::themes::lists::TileArt],
    id: &str,
) -> Option<&'a yapfast_core::themes::lists::TileArt> {
    (!choices.is_empty()).then(|| {
        let seed = yapfast_core::themes::lists::seed(id).unsigned_abs() as usize;
        &choices[seed % choices.len()]
    })
}

pub fn tile_art<'a>(
    ctx: crate::ctx::Ctx<'a>,
    surface: ArtSurface,
    tile: &yapfast_core::themes::lists::TileArt,
    width: f32,
    height: f32,
) -> Option<Element<'a, Message>> {
    tile_art_opacity(ctx, surface, tile, width, height, 1.0)
}

pub fn tile_art_opacity<'a>(
    ctx: crate::ctx::Ctx<'a>,
    surface: ArtSurface,
    tile: &yapfast_core::themes::lists::TileArt,
    width: f32,
    height: f32,
    opacity: f32,
) -> Option<Element<'a, Message>> {
    let image = ctx.themes.list_art(&ctx.look.theme.id, tile, surface)?;
    let (performance, reduced_motion) = ctx.themes.art_motion(&ctx.look.theme.id);
    let running = animation_running(performance, ctx.plays) && ctx.pace.runs();
    Some(
        NineSlice {
            image,
            width,
            height,
            opacity,
            performance,
            reduced_motion,
            running,
            pace: ctx.pace,
        }
        .into(),
    )
}

impl<'a, M: 'a> From<Art<'a>> for Element<'a, M> {
    fn from(art: Art<'a>) -> Self {
        Element::new(art)
    }
}

pub fn surface<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    surface: ArtSurface,
) -> Element<'a, Message> {
    let layers = ctx.themes.art(&ctx.look.theme.id, surface);
    let glass = ctx.themes.glass(&ctx.look.theme.id, surface);
    let (performance, reduced_motion) = ctx.themes.art_motion(&ctx.look.theme.id);
    let running = animation_running(performance, ctx.plays) && ctx.pace.runs();
    let mut content = content.into();
    if let Some(reserve) = ctx.themes.art_reserve(&ctx.look.theme.id, surface) {
        content = iced::widget::container(content)
            .padding(iced::Padding {
                top: reserve.top,
                right: reserve.right,
                bottom: reserve.bottom,
                left: reserve.left,
            })
            .into();
    }
    // A package brush may be a gradient, and the owner's extra surface
    // colours do not fit in Wired's intentionally stable Theme struct.
    // Put the effective owner ground below art and content. When no package
    // paint exists, `surface_paint` is exactly the prior Wired solid.
    let background = ctx.look.theme.surface_paint(surface.ground());
    let base = iced::widget::container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| iced::widget::container::Style {
            background: Some(background),
            ..Default::default()
        });
    // Keep `content` as Stack's intrinsic base and insert decorations under
    // it. Main panes still fill because their content fills; cards, embeds
    // and code blocks retain their natural size instead of expanding to the
    // whole conversation.
    let mut decorated = iced::widget::Stack::with_children([content]);
    if !layers.is_empty() {
        decorated = decorated.push_under(Art {
            layers,
            performance,
            reduced_motion,
            running,
            pace: ctx.pace,
        });
    }
    if let Some(pane) = glass {
        decorated = decorated.push_under(Glass { pane, surface });
    }
    let decorated: Element<'a, Message> = decorated.push_under(base).into();
    let decorated = under_particles(decorated, ctx, surface);
    if surface == ArtSurface::Chat {
        overlay_layers(decorated, ctx, true)
    } else {
        decorated
    }
}

/// Adds an owner theme's deliberately foreground art over a surface. Unlike
/// [`surface`], this does not paint a ground or reserve content room: the
/// package checker only permits `over` art on surfaces whose pictures are
/// cosmetic and take no input, such as the gloss over a profile card.
pub fn surface_over<'a>(
    content: impl Into<Element<'a, Message>>,
    ctx: crate::ctx::Ctx<'a>,
    surface: ArtSurface,
) -> Element<'a, Message> {
    let content = content.into();
    let layers = ctx.themes.art(&ctx.look.theme.id, surface);
    if layers.is_empty() {
        return content;
    }
    let (performance, reduced_motion) = ctx.themes.art_motion(&ctx.look.theme.id);
    let running = animation_running(performance, ctx.plays) && ctx.pace.runs();
    iced::widget::stack![
        content,
        Art {
            layers,
            performance,
            reduced_motion,
            running,
            pace: ctx.pace,
        }
    ]
    .into()
}

/// A regular app dialog whose owner-theme art sits inside the existing kit
/// frame. The kit still owns sizing, focus shielding and Wired fallbacks;
/// this app-side boundary supplies the resolved package pictures and glass.
pub fn framed_dialog<'a>(
    ctx: crate::ctx::Ctx<'a>,
    body: impl IntoIterator<Item = Element<'a, Message>>,
    width: f32,
    close: Message,
) -> Element<'a, Message> {
    let body = crate::widgets::scroll_view(iced::widget::column(body).spacing(12).padding(20))
        .width(Length::Fill)
        .style(crate::style::scrollbar(ctx.look.theme));
    let body = surface(body, ctx, ArtSurface::Dialog);
    crate::dialog::dialog(ctx.look.theme, body, width, close)
}

/// A dialog over server/channel settings, with the same package decoration
/// but the kit's existing dimensions and dimming behavior.
pub fn panel_dialog<'a>(
    ctx: crate::ctx::Ctx<'a>,
    body: impl Into<Element<'a, Message>>,
    width: f32,
    height: Option<f32>,
    close: Message,
) -> Element<'a, Message> {
    let body = surface(body, ctx, ArtSurface::Dialog);
    crate::dialog::panel(ctx.look.theme, body, width, height, close)
}

fn under_particles<'a>(
    content: Element<'a, Message>,
    ctx: crate::ctx::Ctx<'a>,
    surface: ArtSurface,
) -> Element<'a, Message> {
    if surface != ArtSurface::Chat {
        return content;
    }
    let Some((sets, events, performance, reduced_motion)) =
        ctx.themes.particles(&ctx.look.theme.id)
    else {
        return content;
    };
    iced::widget::stack![
        Particles {
            sets,
            events,
            performance,
            reduced_motion,
            running: animation_running(performance, ctx.plays) && ctx.pace.runs(),
            pace: ctx.pace,
            depth: Depth::Under,
        },
        content
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

pub fn overlays<'a>(
    content: Element<'a, Message>,
    ctx: crate::ctx::Ctx<'a>,
) -> Element<'a, Message> {
    overlay_layers(content, ctx, false)
}

fn overlay_layers<'a>(
    content: Element<'a, Message>,
    ctx: crate::ctx::Ctx<'a>,
    chat: bool,
) -> Element<'a, Message> {
    let layers = ctx.themes.overlays(&ctx.look.theme.id, chat);
    let procedural = ctx.themes.procedural_overlays(&ctx.look.theme.id, chat);
    let (performance, reduced_motion) = ctx.themes.art_motion(&ctx.look.theme.id);
    let running = animation_running(performance, ctx.plays) && ctx.pace.runs();
    let particles = (!chat)
        .then(|| ctx.themes.particles(&ctx.look.theme.id))
        .flatten();
    if layers.is_empty() && procedural.is_empty() && particles.is_none() {
        return content;
    }
    let mut decorated = iced::widget::stack![content];
    if !layers.is_empty() {
        decorated = decorated.push(Art {
            layers,
            performance,
            reduced_motion,
            running,
            pace: ctx.pace,
        });
    }
    if !procedural.is_empty() {
        decorated = decorated.push(Procedural {
            overlays: procedural,
            performance,
            reduced_motion,
            running,
            pace: ctx.pace,
        });
    }
    if let Some((sets, events, particle_performance, particle_reduced)) = particles {
        decorated = decorated.push(Particles {
            sets,
            events,
            performance: particle_performance,
            reduced_motion: particle_reduced,
            running: animation_running(particle_performance, ctx.plays) && ctx.pace.runs(),
            pace: ctx.pace,
            depth: Depth::Over,
        });
    }
    decorated.width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    #[test]
    fn balanced_art_pauses_when_the_window_animation_gate_rests() {
        assert!(animation_running(Performance::Full, false));
        assert!(animation_running(Performance::Balanced, true));
        assert!(!animation_running(Performance::Balanced, false));
        assert!(!animation_running(Performance::Light, true));

        let start = Instant::now();
        let mut clock = MotionClock::default();
        clock.redraw(start, true);
        clock.redraw(start + Duration::from_millis(100), true);
        clock.redraw(start + Duration::from_millis(200), false);
        clock.redraw(start + Duration::from_millis(300), true);
        clock.redraw(start + Duration::from_millis(350), true);
        assert_eq!(clock.elapsed, Duration::from_millis(150));
    }
}
