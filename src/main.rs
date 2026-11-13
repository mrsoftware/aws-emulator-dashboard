mod app_state;
mod aws;
mod catalog;
#[cfg(test)]
mod console_render_tests;
mod emulator;
mod settings;
mod views;

use app_state::AppState;
use gpui_kit::application;
use gpui_kit::{
    App, AppContext, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use settings::Settings;
use views::root::{RefreshAll, RootView};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    application().run(|cx: &mut App| {
        let settings = Settings::load();
        let state = cx.new(|cx| AppState::new(settings, cx));
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.0), px(800.0)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("AWS Emulator Dashboard".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    cx.observe(&state, |_, _, cx| cx.notify()).detach();
                    RootView::refresh(&state, cx);
                    RootView::refresh_resources(&state, cx);
                    RootView::start_polling(&state, cx);
                    RootView::new(state.clone())
                })
            },
        )
        .unwrap();
        let adopt_state = state.clone();
        cx.spawn(async move |cx| {
            let (manager, settings) = cx.update(|cx| {
                let s = adopt_state.read(cx);
                (s.manager.clone(), s.settings.clone())
            });
            let adopt_settings = settings.clone();
            let info = aws::client::runtime()
                .spawn(async move { manager.adopt_if_running(&adopt_settings).await })
                .await
                .ok()
                .flatten();
            if let Some(info) = info {
                tracing::info!(container = ?info.container, "adopted running emulator");
                cx.update(|cx| {
                    adopt_state.update(cx, |s, cx| {
                        s.emulator.running = true;
                        s.emulator.startup = Some(info.clone());
                        cx.notify();
                    })
                });
                if settings.proxy_enabled {
                    let proxy = cx.update(|cx| adopt_state.read(cx).proxy.clone());
                    let conn = aws::client::EmulatorConnection::from_startup(
                        info.endpoint_url.clone(),
                        info.config_file.clone(),
                        info.ca_file.clone(),
                    );
                    let result = aws::client::runtime()
                        .spawn(async move { proxy.start(&settings, &conn).await })
                        .await;
                    if let Ok(Ok(_)) = result {
                        cx.update(|cx| {
                            adopt_state.update(cx, |s, cx| {
                                s.proxy_running = true;
                                cx.notify();
                            })
                        });
                    } else {
                        let msg = match result {
                            Ok(Err(e)) => format!("{e:#}"),
                            Err(e) => format!("proxy task failed: {e}"),
                            Ok(Ok(_)) => unreachable!(),
                        };
                        tracing::warn!(error = %msg, "proxy auto-start failed");
                        cx.update(|cx| {
                            adopt_state.update(cx, |s, cx| {
                                s.proxy_error = Some(msg);
                                cx.notify();
                            })
                        });
                    }
                }
            }
        })
        .detach();
        cx.bind_keys([
            KeyBinding::new("cmd-r", RefreshAll, None),
            KeyBinding::new("ctrl-r", RefreshAll, None),
        ]);
        cx.bind_keys([
            KeyBinding::new("backspace", views::text_field::Backspace, Some("TextField")),
            KeyBinding::new("delete", views::text_field::Delete, Some("TextField")),
            KeyBinding::new("left", views::text_field::Left, Some("TextField")),
            KeyBinding::new("right", views::text_field::Right, Some("TextField")),
            KeyBinding::new(
                "shift-left",
                views::text_field::SelectLeft,
                Some("TextField"),
            ),
            KeyBinding::new(
                "shift-right",
                views::text_field::SelectRight,
                Some("TextField"),
            ),
            KeyBinding::new("home", views::text_field::Home, Some("TextField")),
            KeyBinding::new("end", views::text_field::End, Some("TextField")),
            KeyBinding::new("cmd-a", views::text_field::SelectAll, Some("TextField")),
            KeyBinding::new("ctrl-a", views::text_field::SelectAll, Some("TextField")),
            KeyBinding::new("cmd-v", views::text_field::Paste, Some("TextField")),
            KeyBinding::new("ctrl-v", views::text_field::Paste, Some("TextField")),
            KeyBinding::new("cmd-c", views::text_field::Copy, Some("TextField")),
            KeyBinding::new("ctrl-c", views::text_field::Copy, Some("TextField")),
            KeyBinding::new("cmd-x", views::text_field::Cut, Some("TextField")),
            KeyBinding::new("ctrl-x", views::text_field::Cut, Some("TextField")),
        ]);
        cx.activate(true);
    });
}
