//! Refreshes the cached account snapshot on demand.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use norte_code_plugin_host::PluginContext;
use norte_code_wire::plugin_widgets::{Widget, WidgetId, WidgetView};

use crate::usage::{self, Usage, Window};

const FRESH_FOR: Duration = Duration::from_secs(45);
const RETRY_AFTER: Duration = Duration::from_secs(15);
const MAX_STALE: Duration = Duration::from_secs(180);

#[derive(Default)]
pub(super) struct UsageWidget {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    snapshot: Option<(Instant, Usage)>,
    next_attempt: Option<Instant>,
    fetching: bool,
    failed: bool,
}

impl UsageWidget {
    pub(super) fn show(
        &self,
        context: &PluginContext,
        account_id: Option<String>,
        id: WidgetId,
    ) -> WidgetView {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.fetching && state.next_attempt.is_none_or(|next| next <= now) {
            state.fetching = true;
            let shared = Arc::clone(&self.state);
            let context = context.clone();
            let started = std::thread::Builder::new()
                .name("codex-usage".to_owned())
                .spawn(move || {
                    let usage = usage::read().filter(|usage| {
                        account_id
                            .as_ref()
                            .zip(usage.account_id.as_ref())
                            .is_none_or(|(expected, actual)| expected == actual)
                    });
                    shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .finish(usage);
                    context.request_widget_update(&id);
                });
            if started.is_err() {
                state.finish(None);
            }
        }
        state.view(now)
    }
}

impl State {
    fn finish(&mut self, usage: Option<Usage>) {
        let now = Instant::now();
        self.fetching = false;
        self.failed = usage.is_none();
        self.next_attempt = Some(now + if self.failed { RETRY_AFTER } else { FRESH_FOR });
        if let Some(usage) = usage {
            self.snapshot = Some((now, usage));
        }
    }

    fn view(&self, now: Instant) -> WidgetView {
        let widgets = if let Some((fetched, usage)) = self.snapshot.as_ref()
            && now.duration_since(*fetched) < MAX_STALE
        {
            let mut widgets: Vec<_> = [
                ("Primary", usage.rate_limits.primary.as_ref()),
                ("Secondary", usage.rate_limits.secondary.as_ref()),
            ]
            .into_iter()
            .filter_map(|(name, window)| window.map(|window| progress(name, window)))
            .collect();
            if self.failed {
                widgets.push(text("Last known usage"));
            }
            if widgets.is_empty() {
                widgets.push(text("No usage windows"));
            }
            widgets
        } else if self.failed {
            vec![text("Usage unavailable")]
        } else {
            vec![text("Checking usage...")]
        };
        WidgetView {
            title: Some("CODEX USAGE".to_owned()),
            content: widgets,
        }
    }
}

fn progress(fallback: &str, window: &Window) -> Widget {
    let label = match window.duration_minutes {
        Some(300) => "5h",
        Some(10_080) => "Week",
        _ => fallback,
    };
    Widget::Progress {
        label: format!("{label} left"),
        percent: u8::try_from(100 - window.used_percent.clamp(0, 100)).unwrap_or_default(),
        resets_at: window.resets_at,
    }
}

fn text(text: &str) -> Widget {
    Widget::Text {
        text: text.to_owned(),
    }
}

#[cfg(test)]
#[path = "usage_widget_tests.rs"]
mod tests;
