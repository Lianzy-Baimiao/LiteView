//! 文件关联向导：选择是计划，系统查询的结果才是默认状态。

use std::time::{Duration, Instant};

use egui::{RichText, Ui};

use crate::i18n::{fill, tr};
use crate::platform::assoc;
use crate::settings::Settings;
use crate::ui::{theme, widgets};

#[derive(Clone, Default)]
struct State {
    registered: bool,
    defaults: Vec<String>,
    checked_at: Option<Instant>,
    focused: bool,
    only_pending: bool,
    confirm_remove: bool,
    error: Option<String>,
}

impl State {
    fn refresh_due(&self, now: Instant, focused: bool) -> bool {
        self.checked_at.is_none() || focused && (!self.focused || self.checked_at.is_some_and(|at| now.duration_since(at) >= Duration::from_secs(2)))
    }

    fn refresh(&mut self) {
        self.registered = assoc::is_registered();
        self.defaults = assoc::all_exts().into_iter().filter(|ext| assoc::is_default(ext)).collect();
        self.checked_at = Some(Instant::now());
    }

    fn pending(&self, selected: &[String]) -> Vec<String> {
        assoc::selected_exts(selected).into_iter().filter(|ext| !self.defaults.contains(ext)).collect()
    }
}

pub(super) fn show(ui: &mut Ui, s: &mut Settings) -> Option<String> {
    let id = ui.id().with("association-state");
    let mut state = ui.data_mut(|data| data.get_temp::<State>(id)).unwrap_or_default();
    let focused = ui.input(|i| i.focused);
    if state.refresh_due(Instant::now(), focused) {
        state.refresh();
    }
    state.focused = focused;
    if focused {
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
    let msg = content(ui, s, &mut state, tr());
    ui.data_mut(|data| data.insert_temp(id, state));
    msg
}

fn content(ui: &mut Ui, s: &mut Settings, state: &mut State, t: &crate::i18n::Strings) -> Option<String> {
    let p = theme::palette(ui.ctx());
    ui.label(RichText::new(t.a_note).color(p.text_weak).size(12.0));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui.link(t.a_common).clicked() {
            s.assoc_exts = assoc::common_exts();
        }
        if ui.link(t.a_select_all).clicked() {
            s.assoc_exts = assoc::all_exts();
        }
        if ui.link(t.a_clear).clicked() {
            s.assoc_exts.clear();
        }
    });
    let total = assoc::selected_exts(&s.assoc_exts).len();
    let pending = state.pending(&s.assoc_exts);
    let done = total - pending.len();
    ui.add_space(6.0);
    ui.label(RichText::new(fill(t.a_progress, &[&done, &total])).size(16.0).strong());
    ui.add(egui::ProgressBar::new(if total == 0 { 0.0 } else { done as f32 / total as f32 }).desired_width(ui.available_width()).desired_height(5.0).fill(p.accent));
    ui.label(
        RichText::new(if total == 0 {
            t.a_empty.to_owned()
        } else if pending.is_empty() {
            t.a_complete.to_owned()
        } else {
            fill(t.a_pending_count, &[&pending.len()])
        })
        .color(p.text_weak)
        .size(12.0),
    );
    let mut msg = None;
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled_ui(total > 0, |ui| widgets::accent_button(ui, t.a_open_settings, false)).inner.clicked() {
            state.error = match assoc::register(&s.assoc_exts) {
                Ok(()) => assoc::open_default_apps().err().map(|e| fill(t.a_open_failed, &[&e])),
                Err(e) => Some(fill(t.failed, &[&e])),
            };
            state.refresh();
        }
        if ui.link(t.a_refresh).clicked() {
            state.refresh();
        }
    });
    ui.label(RichText::new(t.a_system_steps).color(p.text_weak).size(12.0));
    if let Some(error) = &state.error {
        ui.label(RichText::new(error).color(p.danger));
    }
    ui.add_space(8.0);
    widgets::checkbox(ui, &mut state.only_pending, t.a_only_pending);
    ui.add_space(4.0);
    for (i, (group, name)) in assoc::GROUPS.iter().zip(t.a_groups.split('|')).enumerate() {
        let visible: Vec<_> = group.iter().copied().filter(|ext| !state.only_pending || pending.iter().any(|p| p == ext)).collect();
        if visible.is_empty() {
            continue;
        }
        let count = group.iter().copied().filter(|ext| s.assoc_exts.iter().any(|e| e == ext)).count();
        let label = format!("{name} ({count}/{})", group.len());
        let mut rows = |ui: &mut Ui| {
            ui.horizontal_wrapped(|ui| {
                for &ext in &visible {
                    let mut selected = s.assoc_exts.iter().any(|e| e == ext);
                    let default = state.defaults.iter().any(|e| e == ext);
                    let suffix = if default {
                        t.a_is_default
                    } else if selected {
                        t.a_pending
                    } else {
                        ""
                    };
                    if widgets::checkbox(ui, &mut selected, &format!(".{ext}{suffix}")).changed() {
                        s.assoc_exts.retain(|e| e != ext);
                        if selected {
                            s.assoc_exts.push(ext.to_string());
                        }
                    }
                }
            });
        };
        if state.only_pending {
            ui.label(RichText::new(label).color(p.text_weak).size(12.0));
            rows(ui);
        } else {
            egui::CollapsingHeader::new(label).id_salt(("assoc-group", i)).default_open(i == 0).show(ui, rows);
        }
    }
    ui.add_space(8.0);
    ui.label(RichText::new(if state.registered { t.a_registered } else { t.a_not_registered }).color(p.text_weak).size(12.0));
    if state.registered {
        if ui.link(t.a_unregister).clicked() {
            state.confirm_remove = true;
        }
        if state.confirm_remove {
            ui.label(RichText::new(t.a_remove_note).color(p.text_weak).size(12.0));
            ui.horizontal_wrapped(|ui| {
                if widgets::accent_button(ui, t.a_unregister, true).clicked() {
                    assoc::unregister();
                    state.confirm_remove = false;
                    state.error = None;
                    state.refresh();
                    msg = Some(t.a_removed.to_owned());
                }
                if widgets::plain_button(ui, t.cancel).clicked() {
                    state.confirm_remove = false;
                }
            });
        }
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_is_selection_not_registration() {
        let state = State { defaults: vec!["jpg".into(), "png".into()], ..Default::default() };
        assert_eq!(state.pending(&["png".into(), "webp".into(), "webp".into()]), ["webp"]);
        assert!(state.pending(&[]).is_empty());
        assert!(state.pending(&["jpg".into()]).is_empty());
    }

    #[test]
    fn queries_are_cached_until_focus_returns_or_timer_expires() {
        let now = Instant::now();
        let mut state = State::default();
        assert!(state.refresh_due(now, true));
        state.checked_at = Some(now);
        state.focused = true;
        assert!(!state.refresh_due(now + Duration::from_millis(100), true));
        assert!(!state.refresh_due(now + Duration::from_secs(10), false));
        assert!(state.refresh_due(now + Duration::from_secs(2), true));
        state.focused = false;
        assert!(state.refresh_due(now + Duration::from_millis(100), true));
    }

    #[test]
    fn association_page_wraps_in_both_languages_without_system_actions() {
        for t in [&crate::i18n::ZH, &crate::i18n::EN] {
            for width in [316.0, 490.0] {
                for (exts, defaults, only_pending) in [
                    (assoc::all_exts(), Vec::new(), false),
                    (assoc::all_exts(), Vec::new(), true),
                    (assoc::default_exts(), assoc::all_exts(), true),
                    (Vec::new(), Vec::new(), false),
                ] {
                    let ctx = egui::Context::default();
                    crate::ui::fonts::install(&ctx);
                    crate::ui::theme::install(&ctx);
                    let mut settings = Settings { assoc_exts: exts, ..Default::default() };
                    let mut state = State { defaults, only_pending, registered: true, confirm_remove: true, ..Default::default() };
                    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 2000.0))), ..Default::default() };
                    let _ = ctx.run_ui(input, |ui| {
                        assert!(content(ui, &mut settings, &mut state, t).is_none());
                        assert!(ui.min_rect().width() <= width + 1.0, "content width {} exceeds {width}", ui.min_rect().width());
                    });
                }
            }
        }
    }
}
