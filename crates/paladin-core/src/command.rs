//! Named user actions and the pure `dispatch` that applies them.

use url::Url;

use crate::event::PendingTab;
use crate::model::{Browser, LoadState, Tab, TabId, Window, WindowId};

/// Every user action in Paladin is one of these. The chrome, the keymap,
/// and (in Phase 2) remappable hotkeys all produce `Command`s; nothing else
/// mutates the model from the UI side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    NewTab {
        window: WindowId,
        url: Option<Url>,
    },
    CloseTab(TabId),
    ActivateTab(TabId),
    NextTab(WindowId),
    PreviousTab(WindowId),
    Navigate {
        tab: TabId,
        url: Url,
    },
    Back(TabId),
    Forward(TabId),
    Reload(TabId),
    Stop(TabId),
    FocusAddressBar(WindowId),
    NewWindow,
    CloseWindow(WindowId),
    OpenSettings,
    /// Clear the engine error shown in the status area.
    DismissEngineError,
    Quit,
}

/// What the application must do after [`dispatch`] (or [`crate::apply`])
/// has updated the model. Most variants are forwarded to the engine's tab
/// host; `OpenWindow`, `CloseWindow`, `FocusAddressBar`, `OpenSettings`, and
/// `Quit` are handled by the app itself. Returning these instead of calling
/// the engine directly keeps `dispatch` pure and testable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineAction {
    /// Create and show a new engine tab for `tab` in `window`, loading `url`.
    CreateTab {
        window: WindowId,
        tab: TabId,
        url: Url,
    },
    /// Adopt a tab the engine already created (a `window.open` request) as
    /// `tab` in `window`, and show it.
    AdoptTab {
        window: WindowId,
        tab: TabId,
        pending: PendingTab,
    },
    Navigate {
        tab: TabId,
        url: Url,
    },
    Back(TabId),
    Forward(TabId),
    Reload(TabId),
    Stop(TabId),
    Show(TabId),
    Hide(TabId),
    CloseTab(TabId),
    /// Open a native window for `window` whose first tab is `tab` at `url`.
    OpenWindow {
        window: WindowId,
        tab: TabId,
        url: Url,
    },
    CloseWindow(WindowId),
    FocusAddressBar(WindowId),
    OpenSettings,
    Quit,
}

/// Apply a command to the model and return what the app must do next.
pub fn dispatch(browser: &mut Browser, cmd: &Command) -> Vec<EngineAction> {
    match cmd {
        Command::NewTab { window, url } => new_tab(browser, *window, url.clone()),
        Command::CloseTab(tab) => close_tab(browser, *tab),
        Command::ActivateTab(tab) => activate_tab(browser, *tab),
        Command::NextTab(window) => step_tab(browser, *window, 1),
        Command::PreviousTab(window) => step_tab(browser, *window, -1),
        Command::Navigate { tab, url } => match browser.tab_mut(*tab) {
            Some(t) => {
                t.url = url.clone();
                t.load = LoadState::Loading;
                t.crashed = None;
                vec![EngineAction::Navigate {
                    tab: *tab,
                    url: url.clone(),
                }]
            }
            None => Vec::new(),
        },
        Command::Back(tab) => pass_through(browser, *tab, EngineAction::Back(*tab)),
        Command::Forward(tab) => pass_through(browser, *tab, EngineAction::Forward(*tab)),
        Command::Reload(tab) => match browser.tab_mut(*tab) {
            Some(t) => {
                t.load = LoadState::Loading;
                t.crashed = None;
                vec![EngineAction::Reload(*tab)]
            }
            None => Vec::new(),
        },
        Command::Stop(tab) => pass_through(browser, *tab, EngineAction::Stop(*tab)),
        Command::FocusAddressBar(window) => {
            if browser.window(*window).is_some() {
                vec![EngineAction::FocusAddressBar(*window)]
            } else {
                Vec::new()
            }
        }
        Command::NewWindow => {
            let url = browser.new_tab_url();
            open_window(browser, url)
        }
        Command::CloseWindow(window) => close_window(browser, *window),
        Command::OpenSettings => vec![EngineAction::OpenSettings],
        Command::DismissEngineError => {
            browser.engine_error = None;
            Vec::new()
        }
        Command::Quit => vec![EngineAction::Quit],
    }
}

/// Add a window to the model whose first tab shows `url`. Used by
/// `Command::NewWindow` and by `main` for the initial window, which opens
/// the homepage or a URL given on the command line.
pub fn open_window(browser: &mut Browser, url: Url) -> Vec<EngineAction> {
    let window = browser.allocate_window_id();
    let tab = browser.allocate_tab_id();
    browser.windows.push(Window {
        id: window,
        tabs: vec![Tab::new(tab, url.clone())],
        active: tab,
    });
    vec![EngineAction::OpenWindow { window, tab, url }]
}

fn pass_through(browser: &Browser, tab: TabId, action: EngineAction) -> Vec<EngineAction> {
    if browser.tab(tab).is_some() {
        vec![action]
    } else {
        Vec::new()
    }
}

fn new_tab(browser: &mut Browser, window: WindowId, url: Option<Url>) -> Vec<EngineAction> {
    if browser.window(window).is_none() {
        return Vec::new();
    }
    let url = url.unwrap_or_else(|| browser.new_tab_url());
    let tab = browser.allocate_tab_id();
    let w = browser
        .window_mut(window)
        .expect("window existence checked above");
    let previous = w.active;
    w.tabs.push(Tab::new(tab, url.clone()));
    w.active = tab;
    vec![
        EngineAction::Hide(previous),
        EngineAction::CreateTab { window, tab, url },
    ]
}

/// Remove `tab`. If it was active, the tab to its right becomes active, or
/// the one to its left if it was last. Closing the last tab closes the
/// window, and closing the last window quits.
pub(crate) fn close_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    let mut actions = vec![EngineAction::CloseTab(tab)];
    let new_active = {
        let w = browser
            .window_mut(window_id)
            .expect("window_of_tab returned an existing window");
        let index = w.index_of(tab).expect("tab is in this window");
        w.tabs.remove(index);
        if w.tabs.is_empty() {
            None
        } else if w.active == tab {
            let next = index.min(w.tabs.len() - 1);
            w.active = w.tabs[next].id;
            Some(w.active)
        } else {
            return actions;
        }
    };
    match new_active {
        Some(next) => actions.push(EngineAction::Show(next)),
        None => {
            browser.windows.retain(|w| w.id != window_id);
            actions.push(EngineAction::CloseWindow(window_id));
            if browser.windows.is_empty() {
                actions.push(EngineAction::Quit);
            }
        }
    }
    actions
}

fn activate_tab(browser: &mut Browser, tab: TabId) -> Vec<EngineAction> {
    let Some(window_id) = browser.window_of_tab(tab) else {
        return Vec::new();
    };
    let w = browser
        .window_mut(window_id)
        .expect("window_of_tab returned an existing window");
    if w.active == tab {
        return Vec::new();
    }
    let previous = w.active;
    w.active = tab;
    vec![EngineAction::Hide(previous), EngineAction::Show(tab)]
}

fn step_tab(browser: &mut Browser, window: WindowId, delta: isize) -> Vec<EngineAction> {
    let Some(w) = browser.window(window) else {
        return Vec::new();
    };
    let len = w.tabs.len() as isize;
    let next = (w.active_index() as isize + delta).rem_euclid(len) as usize;
    let target = w.tabs[next].id;
    activate_tab(browser, target)
}

fn close_window(browser: &mut Browser, window: WindowId) -> Vec<EngineAction> {
    let Some(position) = browser.windows.iter().position(|w| w.id == window) else {
        return Vec::new();
    };
    let removed = browser.windows.remove(position);
    let mut actions: Vec<EngineAction> = removed
        .tabs
        .iter()
        .map(|t| EngineAction::CloseTab(t.id))
        .collect();
    actions.push(EngineAction::CloseWindow(window));
    if browser.windows.is_empty() {
        actions.push(EngineAction::Quit);
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_support::{browser, url};

    /// A browser with one window holding one tab. Returns the ids.
    fn one_window() -> (Browser, WindowId, TabId) {
        let mut b = browser();
        let actions = open_window(&mut b, url("https://a.example/"));
        let EngineAction::OpenWindow { window, tab, .. } = actions[0].clone() else {
            panic!("open_window returns OpenWindow");
        };
        (b, window, tab)
    }

    #[test]
    fn open_window_creates_window_with_one_active_tab() {
        let (b, window, tab) = one_window();
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 1);
        assert_eq!(w.active, tab);
        assert_eq!(w.active_tab().url.as_str(), "https://a.example/");
    }

    #[test]
    fn new_tab_uses_new_tab_url_and_becomes_active() {
        let (mut b, window, first) = one_window();
        b.settings.new_tab_url = "https://new.example/".to_string();
        let actions = dispatch(&mut b, &Command::NewTab { window, url: None });
        let w = b.window(window).unwrap();
        assert_eq!(w.tabs.len(), 2);
        let second = w.tabs[1].id;
        assert_eq!(w.active, second);
        assert_eq!(
            actions,
            vec![
                EngineAction::Hide(first),
                EngineAction::CreateTab {
                    window,
                    tab: second,
                    url: url("https://new.example/"),
                },
            ]
        );
    }

    #[test]
    fn new_tab_with_explicit_url_uses_it() {
        let (mut b, window, _) = one_window();
        dispatch(
            &mut b,
            &Command::NewTab {
                window,
                url: Some(url("https://b.example/")),
            },
        );
        assert_eq!(
            b.window(window).unwrap().active_tab().url.as_str(),
            "https://b.example/"
        );
    }

    #[test]
    fn new_tab_in_unknown_window_does_nothing() {
        let (mut b, _, _) = one_window();
        let actions = dispatch(
            &mut b,
            &Command::NewTab {
                window: WindowId(99),
                url: None,
            },
        );
        assert!(actions.is_empty());
        assert_eq!(b.windows.len(), 1);
    }

    #[test]
    fn activate_tab_hides_previous_and_shows_target() {
        let (mut b, window, first) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let second = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::ActivateTab(first));
        assert_eq!(b.window(window).unwrap().active, first);
        assert_eq!(
            actions,
            vec![EngineAction::Hide(second), EngineAction::Show(first)]
        );
        assert!(dispatch(&mut b, &Command::ActivateTab(first)).is_empty());
    }

    #[test]
    fn close_active_tab_activates_tab_to_the_right_then_left() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t3 = b.window(window).unwrap().active;

        dispatch(&mut b, &Command::ActivateTab(t2));
        let actions = dispatch(&mut b, &Command::CloseTab(t2));
        assert_eq!(b.window(window).unwrap().active, t3, "right neighbour wins");
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t2), EngineAction::Show(t3)]
        );

        let actions = dispatch(&mut b, &Command::CloseTab(t3));
        assert_eq!(
            b.window(window).unwrap().active,
            t1,
            "last tab falls back left"
        );
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t3), EngineAction::Show(t1)]
        );
    }

    #[test]
    fn close_inactive_tab_keeps_active_and_emits_no_show() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::CloseTab(t1));
        assert_eq!(b.window(window).unwrap().active, t2);
        assert_eq!(actions, vec![EngineAction::CloseTab(t1)]);
    }

    #[test]
    fn closing_last_tab_closes_window_and_last_window_quits() {
        let (mut b, w1, t1) = one_window();
        let w2_actions = dispatch(&mut b, &Command::NewWindow);
        let EngineAction::OpenWindow {
            window: w2,
            tab: t2,
            ..
        } = w2_actions[0].clone()
        else {
            panic!("NewWindow returns OpenWindow");
        };

        let actions = dispatch(&mut b, &Command::CloseTab(t1));
        assert_eq!(
            actions,
            vec![EngineAction::CloseTab(t1), EngineAction::CloseWindow(w1)]
        );
        assert!(b.window(w1).is_none());

        let actions = dispatch(&mut b, &Command::CloseTab(t2));
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseTab(t2),
                EngineAction::CloseWindow(w2),
                EngineAction::Quit,
            ]
        );
        assert!(b.windows.is_empty());
    }

    #[test]
    fn next_and_previous_tab_wrap_around() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t3 = b.window(window).unwrap().active;

        dispatch(&mut b, &Command::NextTab(window));
        assert_eq!(
            b.window(window).unwrap().active,
            t1,
            "wraps from last to first"
        );
        dispatch(&mut b, &Command::PreviousTab(window));
        assert_eq!(
            b.window(window).unwrap().active,
            t3,
            "wraps from first to last"
        );
        dispatch(&mut b, &Command::PreviousTab(window));
        assert_eq!(b.window(window).unwrap().active, t2);
    }

    #[test]
    fn next_tab_with_single_tab_is_a_no_op() {
        let (mut b, window, t1) = one_window();
        assert!(dispatch(&mut b, &Command::NextTab(window)).is_empty());
        assert_eq!(b.window(window).unwrap().active, t1);
    }

    #[test]
    fn navigate_updates_url_marks_loading_and_clears_crash() {
        let (mut b, _, tab) = one_window();
        b.tab_mut(tab).unwrap().crashed = Some("boom".to_string());
        let actions = dispatch(
            &mut b,
            &Command::Navigate {
                tab,
                url: url("https://b.example/"),
            },
        );
        let t = b.tab(tab).unwrap();
        assert_eq!(t.url.as_str(), "https://b.example/");
        assert_eq!(t.load, LoadState::Loading);
        assert_eq!(t.crashed, None);
        assert_eq!(
            actions,
            vec![EngineAction::Navigate {
                tab,
                url: url("https://b.example/"),
            }]
        );
    }

    #[test]
    fn back_forward_reload_stop_pass_through_for_known_tabs_only() {
        let (mut b, _, tab) = one_window();
        assert_eq!(
            dispatch(&mut b, &Command::Back(tab)),
            vec![EngineAction::Back(tab)]
        );
        assert_eq!(
            dispatch(&mut b, &Command::Forward(tab)),
            vec![EngineAction::Forward(tab)]
        );
        assert_eq!(
            dispatch(&mut b, &Command::Reload(tab)),
            vec![EngineAction::Reload(tab)]
        );
        assert_eq!(b.tab(tab).unwrap().load, LoadState::Loading);
        assert_eq!(
            dispatch(&mut b, &Command::Stop(tab)),
            vec![EngineAction::Stop(tab)]
        );
        assert!(dispatch(&mut b, &Command::Back(TabId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::Reload(TabId(99))).is_empty());
    }

    #[test]
    fn new_window_opens_with_new_tab_url() {
        let (mut b, _, _) = one_window();
        b.settings.new_tab_url = "https://new.example/".to_string();
        let actions = dispatch(&mut b, &Command::NewWindow);
        assert_eq!(b.windows.len(), 2);
        let w = &b.windows[1];
        assert_eq!(
            actions,
            vec![EngineAction::OpenWindow {
                window: w.id,
                tab: w.active,
                url: url("https://new.example/"),
            }]
        );
    }

    #[test]
    fn close_window_closes_all_its_tabs_and_quits_when_last() {
        let (mut b, window, t1) = one_window();
        dispatch(&mut b, &Command::NewTab { window, url: None });
        let t2 = b.window(window).unwrap().active;
        let actions = dispatch(&mut b, &Command::CloseWindow(window));
        assert_eq!(
            actions,
            vec![
                EngineAction::CloseTab(t1),
                EngineAction::CloseTab(t2),
                EngineAction::CloseWindow(window),
                EngineAction::Quit,
            ]
        );
        assert!(b.windows.is_empty());
        assert!(dispatch(&mut b, &Command::CloseWindow(window)).is_empty());
    }

    #[test]
    fn focus_settings_and_quit_are_forwarded_to_the_app() {
        let (mut b, window, _) = one_window();
        assert_eq!(
            dispatch(&mut b, &Command::FocusAddressBar(window)),
            vec![EngineAction::FocusAddressBar(window)]
        );
        assert!(dispatch(&mut b, &Command::FocusAddressBar(WindowId(99))).is_empty());
        assert_eq!(
            dispatch(&mut b, &Command::OpenSettings),
            vec![EngineAction::OpenSettings]
        );
        assert_eq!(dispatch(&mut b, &Command::Quit), vec![EngineAction::Quit]);
    }

    #[test]
    fn tab_and_window_commands_for_unknown_ids_are_no_ops() {
        let (mut b, _, _) = one_window();
        let before = b.windows.clone();
        assert!(dispatch(&mut b, &Command::CloseTab(TabId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::ActivateTab(TabId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::NextTab(WindowId(99))).is_empty());
        assert!(dispatch(&mut b, &Command::PreviousTab(WindowId(99))).is_empty());
        assert_eq!(b.windows, before);
    }

    #[test]
    fn dismiss_engine_error_clears_it() {
        let (mut b, _, _) = one_window();
        b.engine_error = Some("lost backend".to_string());
        assert!(dispatch(&mut b, &Command::DismissEngineError).is_empty());
        assert_eq!(b.engine_error, None);
    }
}
