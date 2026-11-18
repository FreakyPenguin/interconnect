//! Reusable catalog-driven picker. No conversation state, SDK calls, or timers.
use super::{emoji, shell};
use iced::{
    Element, Length,
    widget::{button, column, container, pick_list, row, scrollable, text, text_input, tooltip},
};
use std::sync::OnceLock;

const PAGE_SIZE: usize = 48;
pub const SEARCH_ID: &str = "emoji-picker-search";

/// A shared surface for picker hosts, including any host-specific status text.
pub fn panel<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .id("emoji-picker-panel")
        .padding(12)
        .width(320)
        .height(400)
        .style(surface)
        .into()
}

/// The key is the selected value, independent of the display name and glyph.
/// Custom entries use pack identity as their key and authenticated MXC pixels.
#[derive(Clone)]
pub struct Entry {
    pub custom: Option<interconnect::custom_emoji::Emoji>,
    pub key: String,
    pub name: String,
    pub glyph: String,
    pub category: String,
    pub aliases: Vec<String>,
}
#[derive(Clone)]
pub struct Catalog {
    entries: Vec<Entry>,
    search: Vec<String>,
    categories: Vec<Category>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub custom: Option<interconnect::custom_emoji::Emoji>,
    pub key: String,
    pub shortcode: String,
    pub glyph: String,
}
impl Catalog {
    pub fn with_custom(entries: &[interconnect::custom_emoji::Emoji]) -> Self {
        let mut all: Vec<_> = entries
            .iter()
            .map(|emoji| Entry {
                key: emoji.id.clone(),
                name: format!("{} · {}", emoji.name, emoji.pack),
                glyph: format!(":{}:", emoji.shortcode),
                category: format!("Pack · {}", emoji.pack),
                aliases: vec![emoji.shortcode.clone()],
                custom: Some(emoji.clone()),
            })
            .collect();
        all.extend(unicode().entries.iter().cloned());
        Self::new(all)
    }
    pub fn new(entries: Vec<Entry>) -> Self {
        let mut categories = vec![Category::All];
        for entry in &entries {
            let category = Category::Named(entry.category.clone());
            if !categories.contains(&category) {
                categories.push(category);
            }
        }
        let search = entries
            .iter()
            .map(|entry| {
                format!("{} {} {}", entry.key, entry.name, entry.aliases.join(" "))
                    .to_lowercase()
                    .replace('_', " ")
            })
            .collect();
        Self {
            entries,
            search,
            categories,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.key == key)
    }
    /// Bounded inline results, sharing the catalog's keys and display glyphs.
    pub fn suggest(&self, query: &str, limit: usize) -> Vec<Suggestion> {
        let query = query.to_ascii_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let rank = |alias: &str| {
            if alias == query {
                0
            } else if alias.starts_with(&query) {
                1
            } else {
                2
            }
        };
        let mut matches: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let alias = entry
                    .aliases
                    .iter()
                    .map(|alias| alias.to_ascii_lowercase())
                    .filter(|alias| alias.contains(&query))
                    .min_by(|a, b| (rank(a), a).cmp(&(rank(b), b)))?;
                Some(Suggestion {
                    custom: entry.custom.clone(),
                    key: entry.key.clone(),
                    shortcode: alias,
                    glyph: entry.glyph.clone(),
                })
            })
            .collect();
        matches.sort_by(|a, b| {
            (rank(&a.shortcode), a.custom.is_none(), &a.shortcode).cmp(&(
                rank(&b.shortcode),
                b.custom.is_none(),
                &b.shortcode,
            ))
        });
        matches.truncate(limit);
        matches
    }
}

// Use borders for popup separation: tiny-skia shadows can repaint outside damage.
pub fn surface(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        border: iced::Border {
            color: shell::palette(theme).line,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..shell::surface(theme, shell::palette(theme).background)
    }
}

pub fn unicode() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let popular = [
            "👍", "❤️", "😂", "🎉", "👀", "🙌", "✅", "🔥", "🙏", "😊", "🚀", "💯",
        ];
        let mut entries = Vec::new();
        let all = popular
            .into_iter()
            .filter_map(emojis::get)
            .map(|emoji| (emoji, false))
            .chain(emojis::iter().map(|emoji| (emoji, true)));
        let mut seen = std::collections::HashSet::new();
        for (emoji, include_tones) in all {
            // Include complete skin-tone sequences in the searchable catalog.
            let variants = if include_tones {
                emoji
                    .skin_tones()
                    .map(|tones| tones.collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![emoji])
            } else {
                vec![emoji]
            };
            for emoji in variants {
                if seen.insert(emoji.as_str()) {
                    entries.push(Entry {
                        custom: None,
                        key: emoji.as_str().into(),
                        name: emoji.name().into(),
                        glyph: emoji.as_str().into(),
                        category: match emoji.group() {
                            emojis::Group::SmileysAndEmotion => "Smileys & emotion",
                            emojis::Group::PeopleAndBody => "People & body",
                            emojis::Group::AnimalsAndNature => "Animals & nature",
                            emojis::Group::FoodAndDrink => "Food & drink",
                            emojis::Group::TravelAndPlaces => "Travel & places",
                            emojis::Group::Activities => "Activities",
                            emojis::Group::Objects => "Objects",
                            emojis::Group::Symbols => "Symbols",
                            emojis::Group::Flags => "Flags",
                        }
                        .into(),
                        aliases: emoji.shortcodes().map(str::to_owned).collect(),
                    });
                }
            }
        }
        Catalog::new(entries)
    })
}

pub fn glyph<'a, M: 'static>(
    custom: Option<&interconnect::custom_emoji::Emoji>,
    fallback: &str,
    images: &interconnect::media::Snapshot<interconnect::image_media::Key>,
    size: f32,
) -> Element<'a, M> {
    if let Some(handle) = custom.and_then(|emoji| {
        images
            .ready
            .get(&interconnect::image_media::Key::inline(emoji.uri.clone()))
    }) {
        iced::widget::image(handle.clone())
            .width(size)
            .height(size)
            .content_fit(iced::ContentFit::Contain)
            .into()
    } else if custom.is_some() {
        container(text("◇").size(size))
            .center_x(size)
            .center_y(size)
            .into()
    } else {
        emoji::preview(fallback.to_owned()).size(size).into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Category {
    All,
    Named(String),
}
impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => f.write_str("All emoji"),
            Self::Named(name) => f.write_str(name),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Search(String),
    Category(Category),
    Page(bool),
    Select(String),
    Close,
    Manage,
}

pub struct State {
    query: String,
    category: Category,
    page: usize,
    matches: Vec<usize>,
    revision: u64,
}
impl State {
    pub fn image_keys(&self, catalog: &Catalog) -> Vec<interconnect::image_media::Key> {
        self.visible()
            .filter_map(|index| catalog.entries[index].custom.as_ref())
            .map(|emoji| interconnect::image_media::Key::inline(emoji.uri.clone()))
            .collect()
    }
    pub fn new(catalog: &Catalog) -> Self {
        Self {
            query: String::new(),
            category: Category::All,
            page: 0,
            matches: (0..catalog.entries.len()).collect(),
            revision: 0,
        }
    }
    /// Returns a selected catalog key; the caller decides what selection does.
    pub fn update(&mut self, catalog: &Catalog, event: Event) -> Option<String> {
        match event {
            Event::Select(key) => return catalog.get(&key).map(|entry| entry.key.clone()),
            Event::Search(query) => self.query = query,
            Event::Category(category) => self.category = category,
            Event::Page(forward) => {
                self.page = if forward {
                    (self.page + 1).min(self.pages().saturating_sub(1))
                } else {
                    self.page.saturating_sub(1)
                };
                self.revision += 1;
                return None;
            }
            Event::Close | Event::Manage => return None,
        }
        self.page = 0;
        self.revision += 1;
        let query = self
            .query
            .trim()
            .trim_matches(':')
            .to_lowercase()
            .replace('_', " ");
        let words: Vec<_> = query.split_whitespace().collect();
        self.matches = catalog
            .entries
            .iter()
            .enumerate()
            .filter(|(index, entry)| {
                (match &self.category {
                    Category::All => true,
                    Category::Named(category) => *category == entry.category,
                }) && words
                    .iter()
                    .all(|word| catalog.search[*index].contains(word))
            })
            .map(|(index, _)| index)
            .collect();
        // Exact glyph/shortcode matches come first, including skin-tone sequences.
        self.matches.sort_by_key(|index| {
            let entry = &catalog.entries[*index];
            !(entry.key == query
                || entry
                    .aliases
                    .iter()
                    .any(|alias| alias.replace('_', " ") == query))
        });
        None
    }
    fn pages(&self) -> usize {
        self.matches.len().div_ceil(PAGE_SIZE)
    }
    fn visible(&self) -> impl Iterator<Item = usize> + '_ {
        self.matches
            .iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .copied()
    }
}

fn entry_view<'a, Message: Clone + 'static>(
    entry: &'a Entry,
    images: &'a interconnect::media::Snapshot<interconnect::image_media::Key>,
    select: Option<Message>,
) -> Element<'a, Message> {
    super::tooltips::tooltip(
        button(
            container(glyph(entry.custom.as_ref(), &entry.glyph, images, 22.0))
                .center_x(34)
                .center_y(34)
                .id(format!("emoji-choice:{}", entry.key)),
        )
        .padding(0)
        .style(shell::quiet_button)
        .on_press_maybe(select),
        entry.custom.as_ref().map_or_else(
            || entry.name.clone(),
            |emoji| format!(":{}: · {} · {}", emoji.shortcode, emoji.pack, emoji.name),
        ),
        tooltip::Position::Top,
    )
    .into()
}

/// Embed in any surface; callbacks and busy/error presentation belong to callers.
pub fn view<'a, Message: Clone + 'static>(
    state: &'a State,
    catalog: &'a Catalog,
    images: &'a interconnect::media::Snapshot<interconnect::image_media::Key>,
    enabled: bool,
    on_event: impl Fn(Event) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let search_event = on_event.clone();
    let mut search = text_input("Search emoji", &state.query)
        .id(SEARCH_ID)
        .size(13)
        .padding([7, 10])
        .style(shell::search_style)
        .cursor_blinking(false);
    if enabled {
        search = search.on_input(move |query| search_event(Event::Search(query)));
        if let Some(index) = state.visible().next() {
            search = search.on_submit(on_event(Event::Select(catalog.entries[index].key.clone())));
        }
    }
    let category_event = on_event.clone();
    let categories: Element<'a, Message> = if enabled {
        pick_list(
            catalog.categories.clone(),
            Some(state.category.clone()),
            move |category| category_event(Event::Category(category)),
        )
        .text_size(13)
        .padding([7, 10])
        .style(shell::select_style)
        .into()
    } else {
        container(text(state.category.to_string()).size(12))
            .padding(5)
            .into()
    };
    let grid = row(state.visible().map(|index| {
        let entry = &catalog.entries[index];
        entry_view(
            entry,
            images,
            enabled.then(|| on_event(Event::Select(entry.key.clone()))),
        )
    }))
    .spacing(3)
    .wrap();
    let mut previous = button(text("‹").size(16)).style(shell::quiet_button);
    let mut next = button(text("›").size(16)).style(shell::quiet_button);
    if enabled && state.page > 0 {
        previous = previous.on_press(on_event(Event::Page(false)));
    }
    if enabled && state.page + 1 < state.pages() {
        next = next.on_press(on_event(Event::Page(true)));
    }
    let mut content = column![
        row![
            text("Emoji").size(14),
            iced::widget::space().width(Length::Fill),
            button(super::icons::icon(super::icons::Kind::Close))
                .style(shell::quiet_button)
                .on_press(on_event(Event::Close))
        ]
        .align_y(iced::Alignment::Center),
        search,
        categories,
    ]
    .spacing(8);
    let results: Element<'a, Message> = if state.matches.is_empty() {
        container(text("No emoji found").size(12).style(shell::muted_text))
            .padding(8)
            .into()
    } else {
        grid.into()
    };
    content = content
        .push(
            iced::widget::keyed_column([(
                state.revision,
                Element::from(
                    scrollable(results)
                        .id("emoji-picker-results")
                        .height(Length::Fill)
                        .style(shell::scroll_style),
                ),
            )])
            .height(Length::Fill),
        )
        .push(
            row![
                previous,
                text(format!(
                    "{} / {}",
                    (state.page + 1).min(state.pages()),
                    state.pages()
                ))
                .size(11)
                .style(shell::muted_text),
                next
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    if catalog.entries.iter().any(|entry| entry.custom.is_some()) {
        content = content.push(
            text("Custom emoji images are visible to homeservers.")
                .size(10)
                .style(shell::muted_text),
        );
    }
    if enabled {
        content = content.push(
            button(text("Manage packs…").size(12))
                .style(shell::quiet_button)
                .on_press(on_event(Event::Manage)),
        );
    }
    content.height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{
        Layout, Shell, layout, mouse,
        widget::{self, Tree},
    };

    #[test]
    fn shortcode_suggestions_rank_exact_and_prefix_matches_and_keep_catalog_keys() {
        let catalog = Catalog::new(vec![
            Entry {
                custom: None,
                key: "custom:sea".into(),
                name: "Sea mascot".into(),
                glyph: "★".into(),
                category: "Workspace".into(),
                aliases: vec!["sea".into(), "seaside".into()],
            },
            Entry {
                custom: None,
                key: "suffix".into(),
                name: "Suffix".into(),
                glyph: "x".into(),
                category: "Workspace".into(),
                aliases: vec!["confused".into()],
            },
            Entry {
                custom: None,
                key: "prefix".into(),
                name: "Prefix".into(),
                glyph: "y".into(),
                category: "Workspace".into(),
                aliases: vec!["seal".into()],
            },
        ]);
        let choices = catalog.suggest("SE", 2);
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.shortcode.as_str())
                .collect::<Vec<_>>(),
            ["sea", "seal"]
        );
        assert_eq!(choices[0].key, "custom:sea");
        assert_eq!(choices[0].glyph, "★");
        assert_eq!(catalog.suggest("sea", 6)[0].shortcode, "sea");
        assert_eq!(
            catalog.suggest("se", 6).len(),
            3,
            "one alias per catalog entry"
        );
        assert!(catalog.suggest("", 6).is_empty());
        assert!(catalog.suggest("se", 0).is_empty());
        assert_eq!(unicode().suggest("rocket", 6)[0].key, "🚀");
    }

    #[test]
    fn search_categories_pages_and_exact_unicode_keep_selection_keys_separate() {
        let catalog = unicode();
        let mut state = State::new(catalog);
        assert_eq!(state.visible().count(), PAGE_SIZE);
        state.update(catalog, Event::Page(true));
        assert_eq!(state.page, 1);
        for (query, expected) in [
            (":rocket:", "🚀"),
            ("👍🏽", "👍🏽"),
            ("woman technologist", "👩‍💻"),
        ] {
            state.update(catalog, Event::Search(query.into()));
            assert_eq!(state.page, 0);
            let first = &catalog.entries[state.visible().next().unwrap()];
            assert_eq!(first.key, expected);
        }
        state.update(catalog, Event::Category(Category::Named("Flags".into())));
        assert_eq!(state.visible().count(), 0);
        state.update(catalog, Event::Search(String::new()));
        assert!(
            state
                .matches
                .iter()
                .all(|index| catalog.entries[*index].category == "Flags")
        );
        state.update(catalog, Event::Search("no-such-emoji-1234".into()));
        state.update(catalog, Event::Page(true));
        assert_eq!(state.page, 0);
        assert_eq!(state.pages(), 0);

        let catalog = Catalog::new(vec![Entry {
            custom: None,
            key: "catalog:mascot".into(),
            name: "Our mascot".into(),
            glyph: "★".into(),
            category: "Workspace".into(),
            aliases: vec!["party_mascot".into()],
        }]);
        let mut state = State::new(&catalog);
        state.update(&catalog, Event::Search(":PARTY_mascot:".into()));
        assert_eq!(state.visible().collect::<Vec<_>>(), vec![0]);
        assert_eq!(
            state
                .update(&catalog, Event::Select("catalog:mascot".into()))
                .as_deref(),
            Some("catalog:mascot")
        );
        assert!(
            state
                .update(&catalog, Event::Select("unknown".into()))
                .is_none()
        );
    }

    #[test]
    fn reusable_picker_selects_by_mouse_and_enter_and_has_no_idle_caret_timer() {
        struct Probe(Option<iced::Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("emoji-choice:🚀")) {
                    self.0 = Some(bounds);
                }
            }
        }
        let catalog = unicode();
        let mut state = State::new(catalog);
        state.update(catalog, Event::Search(":rocket:".into()));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for enabled in [true, false] {
            // Using Event as the message type proves this view has no App binding.
            let images = Default::default();
            let mut picker = view(&state, catalog, &images, enabled, |event| event);
            let mut tree = Tree::new(&picker);
            let size = iced::Size::new(296.0, 376.0);
            let node = picker.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(size, size),
            );
            picker.as_widget_mut().operate(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus(SEARCH_ID.into()),
            );
            let mut probe = Probe(None);
            picker
                .as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            let point = probe.0.unwrap().center();
            let mut messages = Vec::new();
            for event in [
                iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                    modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                    physical_key: iced::keyboard::key::Physical::Unidentified(
                        iced::keyboard::key::NativeCode::Unidentified,
                    ),
                    location: iced::keyboard::Location::Standard,
                    modifiers: iced::keyboard::Modifiers::default(),
                    text: None,
                    repeat: false,
                }),
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ] {
                let mut shell = Shell::new(&mut messages);
                picker.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    if matches!(event, iced::Event::Window(_)) {
                        mouse::Cursor::Unavailable
                    } else {
                        mouse::Cursor::Available(point)
                    },
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                if matches!(event, iced::Event::Window(_)) {
                    assert!(!matches!(
                        shell.redraw_request(),
                        iced::window::RedrawRequest::At(_)
                    ));
                }
            }
            assert_eq!(
                messages
                    .iter()
                    .filter(|event| matches!(event, Event::Select(key) if key == "🚀"))
                    .count(),
                2 * usize::from(enabled)
            );
        }
    }
}
