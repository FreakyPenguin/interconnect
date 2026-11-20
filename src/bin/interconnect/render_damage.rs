//! Software-renderer regressions for text damage after navigation.
use super::{App, Message, global_search};
use iced::advanced::{Layout, Renderer as _, layout, mouse, renderer, widget};
use iced::{Color, Font, Rectangle, Size};
use iced_tiny_skia::{
    Layer,
    graphics::{Viewport, damage},
};

fn draw_app(app: &App, tree: &mut widget::Tree, renderer: &mut iced::Renderer, size: Size) {
    let bounds = Rectangle::with_size(size);
    renderer.reset(bounds);
    let mut view = app.view();
    tree.diff(view.as_widget());
    let node = view
        .as_widget_mut()
        .layout(tree, renderer, &layout::Limits::new(size, size));
    view.as_widget().draw(
        tree,
        renderer,
        &super::appearance::theme(app.effective_theme()),
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &bounds,
    );
}

#[test]
fn incremental_search_caption_matches_full_redraw_after_navigation() {
    for size in [Size::new(1400.0, 900.0), Size::new(640.0, 480.0)] {
        let mut app = crate::tests::demo_app();
        let mut renderer = crate::test_renderer(Font::with_name("Inter"), iced::Pixels(14.0));
        let mut tree = widget::Tree::empty();
        let mut old_layers: Option<Vec<Layer>> = None;
        let mut pixels = tiny_skia::Pixmap::new(size.width as u32, size.height as u32).unwrap();
        let mut mask = tiny_skia::Mask::new(size.width as u32, size.height as u32).unwrap();
        let viewport =
            Viewport::with_physical_size(Size::new(size.width as u32, size.height as u32), 1.0);
        let steps = [
            None,
            Some(Message::GlobalSearch(global_search::Event::Open)),
            Some(Message::GlobalSearch(global_search::Event::Close)),
            Some(Message::Room("formatting".into())),
            Some(Message::GlobalSearch(global_search::Event::Open)),
        ];
        for (step, event) in steps.into_iter().enumerate() {
            if let Some(event) = event {
                let _ = app.update(event);
            }
            draw_app(&app, &mut tree, &mut renderer, size);
            let iced::Renderer::Secondary(software) = &mut renderer else {
                unreachable!()
            };
            let current = software.layers().to_vec();
            let bounds = Rectangle::with_size(size);
            let regions = old_layers.as_ref().map_or_else(
                || vec![bounds],
                |old| {
                    damage::group(
                        damage::diff(old, &current, |layer| vec![layer.bounds], Layer::damage),
                        bounds,
                    )
                },
            );
            software.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &regions,
                Color::WHITE,
            );
            let mut expected =
                tiny_skia::Pixmap::new(size.width as u32, size.height as u32).unwrap();
            let mut full_mask =
                tiny_skia::Mask::new(size.width as u32, size.height as u32).unwrap();
            software.draw(
                &mut expected.as_mut(),
                &mut full_mask,
                &viewport,
                &[bounds],
                Color::WHITE,
            );
            // The search caption was previously clipped after "Results" because an
            // empty cached icon text group supplied its translation to the new
            // paragraph's damage. Compare the full caption, including its suffix.
            // An unrelated upstream quad antialias seam can differ by a pixel on
            // input borders; this regression deliberately excludes those borders.
            if step == 1 || step == 4 {
                let caption =
                    current
                        .iter()
                        .flat_map(|layer| &layer.text)
                        .flat_map(|group| group.as_slice())
                        .find_map(|text| match text {
                            iced_tiny_skia::graphics::text::Text::Paragraph {
                                paragraph,
                                position,
                                ..
                            } if paragraph.upgrade().is_some_and(|paragraph| {
                                paragraph.buffer().lines.iter().any(|line| {
                                    line.text().contains("Results stay on this device.")
                                })
                            }) =>
                            {
                                Some(Rectangle::new(*position, paragraph.min_bounds))
                            }
                            _ => None,
                        })
                        .expect("search caption is rendered");
                let left = caption.x.floor() as u32;
                let top = caption.y.floor() as u32;
                let right = (caption.x + caption.width).ceil() as u32;
                let bottom = (caption.y + caption.height).ceil() as u32;
                for y in top..bottom.min(size.height as u32) {
                    for x in left..right.min(size.width as u32) {
                        assert_eq!(
                            pixels.pixel(x, y),
                            expected.pixel(x, y),
                            "{size:?}: caption pixel {x},{y} differs after partial redraw"
                        );
                    }
                }
            }
            old_layers = Some(current);
        }
    }
}

#[test]
fn text_group_damage_uses_both_group_transforms_and_preserves_unchanged_frames() {
    use iced::Transformation;
    use iced_tiny_skia::graphics::text::Text;
    let bounds = Rectangle::with_size(Size::new(1400.0, 900.0));
    let text_bounds = Rectangle {
        x: 10.0,
        y: 20.0,
        width: 80.0,
        height: 20.0,
    };
    let text = Text::Cached {
        content: "caption".into(),
        bounds: text_bounds,
        color: Color::BLACK,
        size: iced::Pixels(14.0),
        line_height: iced::Pixels(20.0),
        font: Font::DEFAULT,
        align_x: iced::advanced::text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: iced::advanced::text::Shaping::Basic,
        clip_bounds: bounds,
    };
    let empty = || Layer {
        bounds,
        quads: vec![],
        primitives: vec![],
        images: vec![],
        text: vec![],
    };
    let old_transform = Transformation::translate(1100.0, 61.0);
    let new_transform = Transformation::translate(400.0, 100.0) * Transformation::scale(2.0);
    let mut previous = empty();
    previous.draw_text_group(vec![text.clone()], bounds, old_transform);
    let mut current = empty();
    current.draw_text_group(vec![text.clone()], bounds, new_transform);
    assert_eq!(
        Layer::damage(&previous, &current),
        vec![text_bounds * old_transform, text_bounds * new_transform]
    );
    assert!(
        Layer::damage(&current, &current).is_empty(),
        "unchanged frames must not add redraw work"
    );

    let mut icon = empty();
    icon.draw_text_cache(std::sync::Arc::from([]), bounds, old_transform);
    let mut caption = empty();
    caption.draw_text_group(vec![text], bounds, Transformation::IDENTITY);
    assert_eq!(Layer::damage(&icon, &caption), vec![text_bounds]);
    assert_eq!(Layer::damage(&caption, &icon), vec![text_bounds]);
}
