//! Static outline icons. Geometry is cached per widget; no animation or wakeups.
use iced::{
    Color, Element, Length, Point, Size, Vector,
    advanced::{self, graphics::geometry::Renderer as _, layout, mouse, renderer, widget},
    widget::canvas::{self, LineCap, LineJoin, Path, Stroke},
};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Search,
    Compass,
    Settings,
    People,
    Home,
    Plus,
    Room,
    RoomAdd,
    Space,
    SpaceAdd,
    PersonAdd,
    PersonRemove,
    More,
    Sidebar,
    Subspaces,
    Pin,
    Unpin,
    Reply,
    Thread,
    Edit,
    Delete,
    Bookmark,
    BookmarkFilled,
    Close,
    Check,
    Save,
    ZoomIn,
    ZoomOut,
    Fit,
    ChevronLeft,
    ArrowLeft,
    Refresh,
    Download,
    OpenExternal,
    Play,
    Stop,
    ChevronDown,
    ChevronUp,
    ChevronRight,
    ArrowUp,
    ArrowDown,
    Smile,
    Poll,
    Grip,
    AddReaction,
    Attach,
    Image,
    Eye,
    Bell,
}

pub(super) fn icon<'a, Message: 'a>(kind: Kind) -> Element<'a, Message> {
    sized(kind, 18.0)
}

pub(super) fn sized<'a, Message: 'a>(kind: Kind, size: f32) -> Element<'a, Message> {
    Element::new(Icon { kind, size })
}

struct Icon {
    kind: Kind,
    size: f32,
}

#[derive(Default)]
struct State {
    cache: canvas::Cache,
    appearance: Cell<Option<(Kind, Color)>>,
}

impl<Message> advanced::Widget<Message, iced::Theme, iced::Renderer> for Icon {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(self.size, self.size, Size::ZERO))
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        style: &renderer::Style,
        layout: advanced::Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        use advanced::Renderer as _;
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        let appearance = (self.kind, style.text_color);
        if state.appearance.replace(Some(appearance)) != Some(appearance) {
            state.cache.clear();
        }
        let geometry = state.cache.draw(renderer, layout.bounds().size(), |frame| {
            frame.scale(self.size / 24.0);
            let stroke = Stroke::default()
                .with_color(style.text_color)
                .with_width(1.8)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round);
            let mut line = |points: &[(f32, f32)]| {
                let path = Path::new(|p| {
                    p.move_to(Point::from(points[0]));
                    for &point in &points[1..] {
                        p.line_to(Point::from(point));
                    }
                });
                frame.stroke(&path, stroke);
            };
            use Kind::*;
            match self.kind {
                Bell => {
                    line(&[(5.0, 17.0), (6.0, 14.0), (6.0, 9.0)]);
                    line(&[(18.0, 9.0), (18.0, 14.0), (19.0, 17.0), (5.0, 17.0)]);
                    line(&[(10.0, 21.0), (14.0, 21.0)]);
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(Point::new(6.0, 9.0));
                            p.bezier_curve_to(
                                Point::new(6.0, 1.0),
                                Point::new(18.0, 1.0),
                                Point::new(18.0, 9.0),
                            );
                        }),
                        stroke,
                    );
                }
                Home => {
                    line(&[(3.0, 10.0), (12.0, 3.0), (21.0, 10.0)]);
                    line(&[
                        (5.0, 9.0),
                        (5.0, 21.0),
                        (10.0, 21.0),
                        (10.0, 14.0),
                        (14.0, 14.0),
                        (14.0, 21.0),
                        (19.0, 21.0),
                        (19.0, 9.0),
                    ]);
                }
                Settings => {
                    line(&[(3.0, 6.0), (6.0, 6.0)]);
                    line(&[(10.0, 6.0), (21.0, 6.0)]);
                    line(&[(3.0, 12.0), (14.0, 12.0)]);
                    line(&[(18.0, 12.0), (21.0, 12.0)]);
                    line(&[(3.0, 18.0), (6.0, 18.0)]);
                    line(&[(10.0, 18.0), (21.0, 18.0)]);
                    for (x, y) in [(8.0, 6.0), (16.0, 12.0), (8.0, 18.0)] {
                        frame.stroke(&Path::circle(Point::new(x, y), 2.0), stroke);
                    }
                }
                Subspaces => {
                    line(&[(12.0, 8.0), (12.0, 12.0)]);
                    line(&[(5.5, 16.0), (5.5, 12.0), (18.5, 12.0), (18.5, 16.0)]);
                    for (x, y, width) in [(8.0, 3.0, 8.0), (2.0, 16.0, 7.0), (15.0, 16.0, 7.0)] {
                        frame.stroke(
                            &Path::rounded_rectangle(
                                Point::new(x, y),
                                Size::new(width, 5.0),
                                1.0.into(),
                            ),
                            stroke,
                        );
                    }
                }
                Image => {
                    line(&[
                        (3.0, 3.0),
                        (21.0, 3.0),
                        (21.0, 21.0),
                        (3.0, 21.0),
                        (3.0, 3.0),
                    ]);
                    line(&[
                        (3.0, 17.0),
                        (9.0, 11.0),
                        (14.0, 16.0),
                        (17.0, 13.0),
                        (21.0, 17.0),
                    ]);
                    frame.stroke(&Path::circle(Point::new(16.0, 8.0), 2.0), stroke);
                }
                Eye => {
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(Point::new(2.0, 12.0));
                            p.quadratic_curve_to(Point::new(12.0, 1.0), Point::new(22.0, 12.0));
                            p.quadratic_curve_to(Point::new(12.0, 23.0), Point::new(2.0, 12.0));
                            p.close();
                        }),
                        stroke,
                    );
                    frame.stroke(&Path::circle(Point::new(12.0, 12.0), 3.0), stroke);
                }
                Plus => {
                    line(&[(5.0, 12.0), (19.0, 12.0)]);
                    line(&[(12.0, 5.0), (12.0, 19.0)]);
                }
                Check => line(&[(5.0, 12.0), (10.0, 17.0), (20.0, 7.0)]),
                Save => {
                    line(&[
                        (4.0, 3.0),
                        (17.0, 3.0),
                        (21.0, 7.0),
                        (21.0, 21.0),
                        (3.0, 21.0),
                        (3.0, 3.0),
                        (4.0, 3.0),
                    ]);
                    line(&[(7.0, 3.0), (7.0, 9.0), (16.0, 9.0), (16.0, 3.0)]);
                    line(&[(7.0, 21.0), (7.0, 14.0), (17.0, 14.0), (17.0, 21.0)]);
                }
                ZoomIn | ZoomOut => {
                    line(&[(15.0, 15.0), (21.0, 21.0)]);
                    line(&[(7.0, 10.0), (13.0, 10.0)]);
                    if self.kind == ZoomIn {
                        line(&[(10.0, 7.0), (10.0, 13.0)]);
                    }
                    frame.stroke(&Path::circle(Point::new(10.0, 10.0), 6.5), stroke);
                }
                Fit => {
                    line(&[(8.0, 3.0), (3.0, 3.0), (3.0, 8.0)]);
                    line(&[(16.0, 3.0), (21.0, 3.0), (21.0, 8.0)]);
                    line(&[(3.0, 16.0), (3.0, 21.0), (8.0, 21.0)]);
                    line(&[(16.0, 21.0), (21.0, 21.0), (21.0, 16.0)]);
                    frame.stroke(
                        &Path::rounded_rectangle(
                            Point::new(7.0, 7.0),
                            Size::new(10.0, 10.0),
                            1.0.into(),
                        ),
                        stroke,
                    );
                }

                Close => {
                    line(&[(6.0, 6.0), (18.0, 18.0)]);
                    line(&[(18.0, 6.0), (6.0, 18.0)]);
                }
                ChevronLeft => line(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]),
                ArrowLeft => {
                    line(&[(10.0, 5.0), (3.0, 12.0), (10.0, 19.0)]);
                    line(&[(3.0, 12.0), (21.0, 12.0)]);
                }
                Refresh => {
                    line(&[(21.0, 4.0), (21.0, 10.0), (15.0, 10.0)]);
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(Point::new(21.0, 10.0));
                            p.bezier_curve_to(
                                Point::new(18.0, -1.0),
                                Point::new(3.0, 2.0),
                                Point::new(3.0, 12.0),
                            );
                            p.bezier_curve_to(
                                Point::new(3.0, 22.0),
                                Point::new(18.0, 25.0),
                                Point::new(21.0, 15.0),
                            );
                        }),
                        stroke,
                    );
                }
                Download => {
                    line(&[(12.0, 3.0), (12.0, 15.0)]);
                    line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
                    line(&[(4.0, 16.0), (4.0, 21.0), (20.0, 21.0), (20.0, 16.0)]);
                }
                OpenExternal => {
                    line(&[(14.0, 3.0), (21.0, 3.0), (21.0, 10.0)]);
                    line(&[(21.0, 3.0), (10.0, 14.0)]);
                    line(&[
                        (10.0, 4.0),
                        (4.0, 4.0),
                        (4.0, 20.0),
                        (20.0, 20.0),
                        (20.0, 14.0),
                    ]);
                }
                Play => line(&[(7.0, 3.0), (21.0, 12.0), (7.0, 21.0), (7.0, 3.0)]),
                Stop => frame.stroke(
                    &Path::rectangle(Point::new(5.0, 5.0), Size::new(14.0, 14.0)),
                    stroke,
                ),
                ChevronDown => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
                ChevronUp => line(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)]),
                ChevronRight => line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
                ArrowUp | ArrowDown => {
                    let (a, b) = if self.kind == ArrowUp {
                        (5.0, 19.0)
                    } else {
                        (19.0, 5.0)
                    };
                    let mid = if self.kind == ArrowUp { 11.0 } else { 13.0 };
                    line(&[(6.0, mid), (12.0, a), (18.0, mid)]);
                    line(&[(12.0, a), (12.0, b)]);
                }
                Thread => {
                    line(&[
                        (4.0, 4.0),
                        (20.0, 4.0),
                        (20.0, 16.0),
                        (11.0, 16.0),
                        (6.0, 21.0),
                        (6.0, 16.0),
                        (4.0, 16.0),
                        (4.0, 4.0),
                    ]);
                    line(&[(8.0, 8.0), (16.0, 8.0)]);
                    line(&[(8.0, 12.0), (14.0, 12.0)]);
                }
                Reply => {
                    line(&[(9.0, 5.0), (3.0, 11.0), (9.0, 17.0)]);
                    line(&[(3.0, 11.0), (15.0, 11.0), (19.0, 15.0), (19.0, 20.0)]);
                }
                Delete => {
                    line(&[(4.0, 6.0), (20.0, 6.0)]);
                    line(&[(9.0, 6.0), (9.0, 3.0), (15.0, 3.0), (15.0, 6.0)]);
                    line(&[(6.0, 6.0), (7.0, 21.0), (17.0, 21.0), (18.0, 6.0)]);
                    line(&[(10.0, 10.0), (10.0, 17.0)]);
                    line(&[(14.0, 10.0), (14.0, 17.0)]);
                }
                Edit => {
                    line(&[
                        (4.0, 20.0),
                        (5.0, 16.0),
                        (17.0, 4.0),
                        (20.0, 7.0),
                        (8.0, 19.0),
                        (4.0, 20.0),
                    ]);
                    line(&[(14.5, 6.5), (17.5, 9.5)]);
                }
                Pin | Unpin => {
                    line(&[
                        (9.0, 3.0),
                        (15.0, 3.0),
                        (14.0, 10.0),
                        (18.0, 14.0),
                        (6.0, 14.0),
                        (10.0, 10.0),
                        (9.0, 3.0),
                    ]);
                    line(&[(12.0, 14.0), (12.0, 21.0)]);
                    if self.kind == Unpin {
                        line(&[(3.0, 3.0), (21.0, 21.0)]);
                    }
                }
                Bookmark | BookmarkFilled => {
                    let path = Path::new(|p| {
                        p.move_to(Point::new(6.0, 3.0));
                        for point in [(18.0, 3.0), (18.0, 21.0), (12.0, 17.0), (6.0, 21.0)] {
                            p.line_to(Point::from(point));
                        }
                        p.close();
                    });
                    if self.kind == BookmarkFilled {
                        frame.fill(&path, style.text_color);
                    } else {
                        frame.stroke(&path, stroke);
                    }
                }
                Compass => {
                    line(&[
                        (16.0, 8.0),
                        (14.0, 14.0),
                        (8.0, 16.0),
                        (10.0, 10.0),
                        (16.0, 8.0),
                    ]);
                    frame.stroke(&Path::circle(Point::new(12.0, 12.0), 9.0), stroke);
                }
                Search => {
                    line(&[(16.0, 16.0), (21.0, 21.0)]);
                    frame.stroke(&Path::circle(Point::new(10.5, 10.5), 6.5), stroke);
                }
                Sidebar => {
                    line(&[(9.0, 4.0), (9.0, 20.0)]);
                    frame.stroke(
                        &Path::rounded_rectangle(
                            Point::new(3.0, 4.0),
                            Size::new(18.0, 16.0),
                            2.0.into(),
                        ),
                        stroke,
                    );
                }
                Space | SpaceAdd => {
                    if self.kind == SpaceAdd {
                        line(&[(18.0, 14.0), (18.0, 22.0)]);
                        line(&[(14.0, 18.0), (22.0, 18.0)]);
                    }
                    let (center, radius) = if self.kind == SpaceAdd {
                        (10.0, 7.0)
                    } else {
                        (12.0, 9.0)
                    };
                    line(&[(center - radius, center), (center + radius, center)]);
                    let point = |x: f32, y: f32| {
                        Point::new(center + x * radius / 7.0, center + y * radius / 7.0)
                    };
                    frame.stroke(&Path::circle(Point::new(center, center), radius), stroke);
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(point(0.0, -7.0));
                            p.bezier_curve_to(point(-4.0, -3.0), point(-4.0, 3.0), point(0.0, 7.0));
                            p.bezier_curve_to(point(4.0, 3.0), point(4.0, -3.0), point(0.0, -7.0));
                        }),
                        stroke,
                    );
                }
                PersonAdd | PersonRemove => {
                    line(&[(15.0, 17.0), (23.0, 17.0)]);
                    if self.kind == PersonAdd {
                        line(&[(19.0, 13.0), (19.0, 21.0)]);
                    }
                    frame.stroke(&Path::circle(Point::new(9.0, 7.0), 3.0), stroke);
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(Point::new(2.0, 21.0));
                            p.line_to(Point::new(2.0, 18.0));
                            p.bezier_curve_to(
                                Point::new(2.0, 11.0),
                                Point::new(14.0, 11.0),
                                Point::new(14.0, 16.0),
                            );
                        }),
                        stroke,
                    );
                }
                Room | RoomAdd => {
                    line(&[(8.0, 3.0), (5.0, 21.0)]);
                    line(&[
                        (16.0, 3.0),
                        if self.kind == RoomAdd {
                            (14.5, 12.0)
                        } else {
                            (13.0, 21.0)
                        },
                    ]);
                    line(&[(3.0, 9.0), (20.0, 9.0)]);
                    line(&[
                        (2.0, 15.0),
                        if self.kind == RoomAdd {
                            (10.0, 15.0)
                        } else {
                            (20.0, 15.0)
                        },
                    ]);
                    if self.kind == RoomAdd {
                        line(&[(17.0, 14.0), (17.0, 22.0)]);
                        line(&[(13.0, 18.0), (21.0, 18.0)]);
                    }
                }
                More => {
                    for x in [5.0, 12.0, 19.0] {
                        frame.fill(&Path::circle(Point::new(x, 12.0), 1.3), style.text_color);
                    }
                }
                People => {
                    frame.stroke(&Path::circle(Point::new(9.0, 7.0), 3.0), stroke);
                    frame.stroke(&Path::circle(Point::new(18.0, 8.0), 2.5), stroke);
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(Point::new(3.0, 21.0));
                            p.line_to(Point::new(3.0, 18.0));
                            p.bezier_curve_to(
                                Point::new(3.0, 11.0),
                                Point::new(15.0, 11.0),
                                Point::new(15.0, 18.0),
                            );
                            p.line_to(Point::new(15.0, 21.0));
                            p.close();
                            p.move_to(Point::new(18.0, 14.0));
                            p.bezier_curve_to(
                                Point::new(22.0, 14.0),
                                Point::new(22.0, 17.0),
                                Point::new(22.0, 21.0),
                            );
                            p.line_to(Point::new(19.0, 21.0));
                        }),
                        stroke,
                    );
                }
                Smile | AddReaction => {
                    frame.with_save(|frame| {
                        if self.kind == AddReaction {
                            frame.translate(Vector::new(0.0, 4.0));
                            frame.scale(0.75_f32);
                        }
                        frame.stroke(&Path::circle(Point::new(12.0, 12.0), 9.0), stroke);
                        for x in [9.0, 15.0] {
                            frame.fill(&Path::circle(Point::new(x, 9.0), 0.9), style.text_color);
                        }
                        frame.stroke(
                            &Path::new(|p| {
                                p.move_to(Point::new(8.0, 14.0));
                                p.quadratic_curve_to(
                                    Point::new(12.0, 19.0),
                                    Point::new(16.0, 14.0),
                                );
                            }),
                            stroke,
                        );
                    });
                    if self.kind == AddReaction {
                        frame.stroke(
                            &Path::line(Point::new(17.0, 4.0), Point::new(23.0, 4.0)),
                            stroke,
                        );
                        frame.stroke(
                            &Path::line(Point::new(20.0, 1.0), Point::new(20.0, 7.0)),
                            stroke,
                        );
                    }
                }
                Grip => {
                    for x in [9.0, 15.0] {
                        for y in [6.0, 12.0, 18.0] {
                            frame.fill(&Path::circle(Point::new(x, y), 1.3), style.text_color);
                        }
                    }
                }
                Poll => {
                    for (x, top) in [(8.0, 12.0), (12.0, 7.0), (16.0, 10.0)] {
                        line(&[(x, top), (x, 17.0)]);
                    }
                    frame.stroke(
                        &Path::rounded_rectangle(
                            Point::new(3.0, 3.0),
                            Size::new(18.0, 18.0),
                            3.0.into(),
                        ),
                        stroke,
                    );
                }
                Attach => {
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(Point::new(8.0, 13.0));
                            p.line_to(Point::new(14.0, 7.0));
                            p.bezier_curve_to(
                                Point::new(17.0, 4.0),
                                Point::new(21.0, 8.0),
                                Point::new(18.0, 11.0),
                            );
                            p.line_to(Point::new(10.0, 19.0));
                            p.bezier_curve_to(
                                Point::new(5.0, 24.0),
                                Point::new(-1.0, 18.0),
                                Point::new(4.0, 13.0),
                            );
                            p.line_to(Point::new(13.0, 4.0));
                        }),
                        stroke,
                    );
                }
            }
        });
        renderer.with_translation(
            Vector::new(layout.position().x, layout.position().y),
            |renderer| {
                renderer.draw_geometry(geometry);
            },
        );
    }
}
