//! The window chrome: tab strip, toolbar, status overlay, and crash panel.
//! Pure functions of the model that return the commands the user issued.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

use egui::{Button, ColorImage, Id, Key, Panel, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use paladin_core::{Command, Favicon, LoadState, TabId, Window};
use url::{ParseError, Url};

const TAB_TITLE_MAX_CHARS: usize = 24;
const FAVICON_SIZE: f32 = 16.0;

/// Per-window chrome state that is not part of the browser model: the
/// address bar text the user may be editing, and favicon textures.
#[derive(Default)]
pub struct ChromeState {
    pub address_text: String,
    /// True while the user has edited the address bar since the last
    /// navigation; engine URL changes then leave the text alone.
    pub address_dirty: bool,
    /// Set by `Command::FocusAddressBar`; consumed by the next frame.
    pub focus_address: bool,
    shown_tab: Option<TabId>,
    shown_url: Option<Url>,
    favicons: HashMap<TabId, CachedFavicon>,
}

/// An uploaded favicon texture. The `Arc` is a clone of the model's pixels:
/// holding it keeps that allocation alive, so pointer equality with a new
/// favicon's pixels means the same icon, never a reused address.
struct CachedFavicon {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
    texture: TextureHandle,
}

/// Draw the chrome for `window` and return the commands it produced.
/// `engine_error` is the browser-wide engine error to show in the status
/// area, if any.
pub fn draw(
    ui: &mut Ui,
    window: &Window,
    engine_error: Option<&str>,
    state: &mut ChromeState,
) -> Vec<Command> {
    let mut commands = Vec::new();
    let active = window.active_tab();
    sync_address_text(state, active.id, &active.url);
    state
        .favicons
        .retain(|tab, _| window.index_of(*tab).is_some());

    Panel::top("tabs").show_inside(ui, |ui| {
        egui::ScrollArea::horizontal()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tab in &window.tabs {
                        let is_active = tab.id == window.active;
                        if let Some(texture) =
                            favicon_texture(ui, state, tab.id, tab.favicon.as_ref())
                        {
                            ui.add(egui::Image::new((texture.id(), Vec2::splat(FAVICON_SIZE))));
                        }
                        let title = truncate(tab.display_title(), TAB_TITLE_MAX_CHARS);
                        let response = ui.add(Button::selectable(is_active, title));
                        if response.clicked() && !is_active {
                            commands.push(Command::ActivateTab(tab.id));
                        }
                        if response.middle_clicked() {
                            commands.push(Command::CloseTab(tab.id));
                        }
                        if ui.small_button("×").on_hover_text("Close tab").clicked() {
                            commands.push(Command::CloseTab(tab.id));
                        }
                        ui.add_space(4.0);
                    }
                    if ui.button("+").on_hover_text("New tab").clicked() {
                        commands.push(Command::NewTab {
                            window: window.id,
                            url: None,
                        });
                    }
                });
            });
    });

    Panel::top("toolbar").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(active.can_go_back, Button::new("◀"))
                .on_hover_text("Back")
                .clicked()
            {
                commands.push(Command::Back(active.id));
            }
            if ui
                .add_enabled(active.can_go_forward, Button::new("▶"))
                .on_hover_text("Forward")
                .clicked()
            {
                commands.push(Command::Forward(active.id));
            }
            // Reload is always available (a hung load must be recoverable);
            // Stop appears beside it while loading.
            if ui.button("⟳").on_hover_text("Reload").clicked() {
                state.address_dirty = false;
                commands.push(Command::Reload(active.id));
            }
            // Servo 0.7.0 cannot stop a load; the button shows the state but
            // stays disabled so the UI does not promise what the engine lacks.
            if active.load == LoadState::Loading {
                ui.add_enabled(false, Button::new("✕"))
                    .on_disabled_hover_text("Stop (not supported by Servo 0.7.0)");
            }

            let address_id = Id::new(("address", window.id));
            let field = ui.add_sized(
                ui.available_size(),
                egui::TextEdit::singleline(&mut state.address_text)
                    .id(address_id)
                    .hint_text("Enter address"),
            );
            if field.changed() {
                state.address_dirty = true;
            }
            // Select the text on focus, and again on Cmd+L while already
            // focused, so the shortcut always means "replace the address".
            let focus_requested = state.focus_address;
            if state.focus_address {
                state.focus_address = false;
                field.request_focus();
            }
            if field.gained_focus() || focus_requested {
                select_all(ui, address_id, state.address_text.chars().count());
            }
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(Key::Enter))
                && let Some(url) = location_input_to_url(&state.address_text)
            {
                state.address_dirty = false;
                commands.push(Command::Navigate {
                    tab: active.id,
                    url,
                });
            }
        });
    });

    // The status text floats over the bottom-left corner of the page as an
    // overlay, so showing or hiding it never resizes the page.
    let status = match engine_error {
        Some(error) => Some(format!("Engine error: {error}")),
        None if !active.status.is_empty() => Some(active.status.clone()),
        None => None,
    };
    if let Some(text) = status {
        egui::Area::new(Id::new(("status", window.id)))
            .anchor(egui::Align2::LEFT_BOTTOM, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(engine_error.is_some())
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(text).small());
                        if engine_error.is_some()
                            && ui.small_button("×").on_hover_text("Dismiss").clicked()
                        {
                            commands.push(Command::DismissEngineError);
                        }
                    });
                });
            });
    }

    commands
}

/// Replace the page area with an error panel. Returns true if the user
/// asked to reload.
pub fn draw_crash_panel(ui: &mut Ui, rect: Rect, reason: &str) -> bool {
    let mut reload = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("This tab crashed");
            ui.label(reason);
            ui.add_space(12.0);
            if ui.button("Reload").clicked() {
                reload = true;
            }
        });
    });
    reload
}

/// Turn address-bar text into a URL. Bare domains get `https://`. `localhost`,
/// bare IPv4 addresses, and `host:port` get `http://`. `javascript:` URLs
/// are refused. Returns `None` for text that is not a URL (Phase 1 has no
/// search engine).
pub fn location_input_to_url(input: &str) -> Option<Url> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    match Url::parse(text) {
        Ok(url) if url.scheme() == "javascript" => None,
        // "localhost:8080" and "example.com:8080" parse as scheme + path.
        Ok(url)
            if url.host_str().is_none()
                && (url.scheme() == "localhost" || url.scheme().contains('.')) =>
        {
            with_default_scheme(text)
        }
        Ok(url) => Some(url),
        Err(ParseError::RelativeUrlWithoutBase) if looks_like_host(text) => {
            with_default_scheme(text)
        }
        Err(_) => None,
    }
}

/// `text` with the scheme `default_scheme` picks for it, parsed.
fn with_default_scheme(text: &str) -> Option<Url> {
    Url::parse(&format!("{}://{text}", default_scheme(text))).ok()
}

/// `http` for `localhost`, bare IPv4 addresses, and `host:port`; `https` for
/// everything else.
fn default_scheme(text: &str) -> &'static str {
    let (host, port) = split_port(authority(text));
    if host == "localhost" || host.parse::<Ipv4Addr>().is_ok() || port.is_some() {
        "http"
    } else {
        "https"
    }
}

fn looks_like_host(text: &str) -> bool {
    let (host, _) = split_port(authority(text));
    !text.contains(char::is_whitespace)
        && !text.starts_with('.')
        && (host == "localhost" || host.contains('.'))
}

/// The part of address-bar text before any path, query, or fragment, such as
/// `host` or `host:port`.
fn authority(text: &str) -> &str {
    text.split(['/', '?', '#']).next().unwrap_or(text)
}

/// Splits `host:port` when the part after the last colon is all digits.
fn split_port(authority: &str) -> (&str, Option<&str>) {
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (authority, None),
    }
}

fn sync_address_text(state: &mut ChromeState, tab: TabId, url: &Url) {
    let tab_changed = state.shown_tab != Some(tab);
    let url_changed = state.shown_url.as_ref() != Some(url);
    if tab_changed || (url_changed && !state.address_dirty) {
        state.address_text = url.to_string();
        state.address_dirty = false;
    }
    state.shown_tab = Some(tab);
    state.shown_url = Some(url.clone());
}

fn select_all(ui: &Ui, id: Id, len: usize) {
    use egui::text::{CCursor, CCursorRange};
    use egui::text_edit::TextEditState;
    if let Some(mut edit) = TextEditState::load(ui.ctx(), id) {
        edit.cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
        edit.store(ui.ctx(), id);
    }
}

fn favicon_texture<'a>(
    ui: &Ui,
    state: &'a mut ChromeState,
    tab: TabId,
    favicon: Option<&Favicon>,
) -> Option<&'a TextureHandle> {
    let favicon = favicon?;
    let fresh = state
        .favicons
        .get(&tab)
        .is_some_and(|cached| cached.is_for(favicon));
    if !fresh {
        let image = ColorImage::from_rgba_unmultiplied(
            [favicon.width as usize, favicon.height as usize],
            &favicon.rgba,
        );
        let texture =
            ui.ctx()
                .load_texture(format!("favicon-{}", tab.0), image, TextureOptions::LINEAR);
        state.favicons.insert(
            tab,
            CachedFavicon {
                width: favicon.width,
                height: favicon.height,
                rgba: favicon.rgba.clone(),
                texture,
            },
        );
    }
    state.favicons.get(&tab).map(|cached| &cached.texture)
}

impl CachedFavicon {
    /// True if this texture was uploaded from `favicon`'s pixels. The model
    /// replaces the `Arc` whenever the engine reports a new icon, so the
    /// pointer comparison is exact and costs nothing per frame.
    fn is_for(&self, favicon: &Favicon) -> bool {
        self.width == favicon.width
            && self.height == favicon.height
            && Arc::ptr_eq(&self.rgba, &favicon.rgba)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(
            location_input_to_url("https://example.com/a?b=c")
                .unwrap()
                .as_str(),
            "https://example.com/a?b=c"
        );
        assert_eq!(
            location_input_to_url("about:blank").unwrap().as_str(),
            "about:blank"
        );
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(
            location_input_to_url("example.com").unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            location_input_to_url("  www.example.com/path ")
                .unwrap()
                .as_str(),
            "https://www.example.com/path"
        );
    }

    #[test]
    fn localhost_and_host_port_get_http() {
        assert_eq!(
            location_input_to_url("localhost").unwrap().as_str(),
            "http://localhost/"
        );
        assert_eq!(
            location_input_to_url("localhost:8080").unwrap().as_str(),
            "http://localhost:8080/"
        );
        assert_eq!(
            location_input_to_url("example.com:8080").unwrap().as_str(),
            "http://example.com:8080/"
        );
    }

    #[test]
    fn non_urls_are_rejected() {
        assert_eq!(location_input_to_url(""), None);
        assert_eq!(location_input_to_url("hello world"), None);
        assert_eq!(location_input_to_url("justaword"), None);
        assert_eq!(location_input_to_url(".hidden"), None);
    }

    #[test]
    fn localhost_with_a_path_gets_http() {
        assert_eq!(
            location_input_to_url("localhost/path").unwrap().as_str(),
            "http://localhost/path"
        );
        assert_eq!(
            location_input_to_url("localhost:8080/path?q=1")
                .unwrap()
                .as_str(),
            "http://localhost:8080/path?q=1"
        );
    }

    #[test]
    fn a_domain_that_starts_with_localhost_gets_https() {
        assert_eq!(
            location_input_to_url("localhostfoo.com").unwrap().as_str(),
            "https://localhostfoo.com/"
        );
    }

    #[test]
    fn bare_ipv4_addresses_get_http_with_or_without_port_and_path() {
        assert_eq!(
            location_input_to_url("192.168.1.1").unwrap().as_str(),
            "http://192.168.1.1/"
        );
        assert_eq!(
            location_input_to_url("192.168.1.1:8080").unwrap().as_str(),
            "http://192.168.1.1:8080/"
        );
        assert_eq!(
            location_input_to_url("10.0.0.1/admin").unwrap().as_str(),
            "http://10.0.0.1/admin"
        );
    }

    #[test]
    fn javascript_urls_are_refused() {
        assert_eq!(location_input_to_url("javascript:alert(1)"), None);
        assert_eq!(location_input_to_url("  JavaScript:void(0) "), None);
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn edited_address_text_survives_an_engine_url_change() {
        let mut state = ChromeState::default();
        sync_address_text(&mut state, TabId(1), &url("https://a.test/"));
        state.address_text = "typed by the user".into();
        state.address_dirty = true;
        sync_address_text(&mut state, TabId(1), &url("https://a.test/moved"));
        assert_eq!(state.address_text, "typed by the user");
        assert!(state.address_dirty);
    }

    #[test]
    fn switching_tabs_resets_edited_text_to_the_new_tab_url() {
        let mut state = ChromeState::default();
        sync_address_text(&mut state, TabId(1), &url("https://a.test/"));
        state.address_text = "typed by the user".into();
        state.address_dirty = true;
        sync_address_text(&mut state, TabId(2), &url("https://b.test/"));
        assert_eq!(state.address_text, "https://b.test/");
        assert!(!state.address_dirty);
    }

    #[test]
    fn clean_address_text_follows_the_url() {
        let mut state = ChromeState::default();
        sync_address_text(&mut state, TabId(1), &url("https://a.test/"));
        assert!(!state.address_dirty);
        sync_address_text(&mut state, TabId(1), &url("https://a.test/next"));
        assert_eq!(state.address_text, "https://a.test/next");
        assert!(!state.address_dirty);
    }

    #[test]
    fn truncate_adds_ellipsis_only_when_cut() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a longer title here", 8), "a longer…");
    }
}
