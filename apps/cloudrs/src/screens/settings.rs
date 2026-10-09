//! The Settings screen (ADR 0017): theme, the audio output (ADR 0020), sound
//! (ADR 0022), Discord, the artwork cache, updates (ADR 0026) and the keyboard
//! shortcuts. Every change goes to the core as a whole `Settings`
//! and takes effect when the core echoes it.

use cloudrs_ui::Theme;
use cloudrs_ui::components::{ButtonKind, button, pill};
use cloudrs_ui::tokens::{radius, size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, div};
use sc_core::{Command, EqPreset, Settings, ThemeChoice};
use sc_platform::update::Progress;

use super::account::muted;
use crate::i18n::{app, discord as d, settings as t, update as u};
use crate::shell::updates::{UpdateState, local_time, status_text};
use crate::shell::{Shell, shortcuts};

/// Megabytes with one decimal, for the cache size.
fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_048_576.0)
}

impl Shell {
    pub(crate) fn settings_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px(space::S5)
            .pb(space::S6)
            .child(
                theme
                    .text(div(), typography::DISPLAY_L)
                    .pt(space::S2)
                    .pb(space::S4)
                    .text_color(c.text)
                    .child(t::title()),
            )
            .child(self.theme_setting(theme, cx))
            .child(self.output_setting(theme, cx))
            .child(self.sound_setting(theme, cx))
            .children(self.discord_setting(theme, cx))
            // The language picker appears with a second language (ADR 0017).
            .child(self.cache_setting(theme, cx))
            .child(self.update_setting(theme, cx))
            .child(shortcut_list(theme))
            .into_any_element()
    }

    /// System, Dark or Light.
    fn theme_setting(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let selected = self.models.settings.theme;
        let choice = |ix: usize, label: &'static str, value: ThemeChoice| {
            pill(theme, ("theme", ix), label, selected == value)
                .tab_index(0)
                .aria_label(label)
                .on_click(cx.listener(move |this, _, _, _| {
                    this.change_settings(|settings| settings.theme = value);
                }))
        };
        section(theme, t::theme(), t::theme_hint()).child(
            div()
                .flex()
                .gap(space::S2)
                .pt(space::S1)
                .child(choice(0, t::theme_system(), ThemeChoice::System))
                .child(choice(1, t::theme_dark(), ThemeChoice::Dark))
                .child(choice(2, t::theme_light(), ThemeChoice::Light)),
        )
    }

    /// System default, then one pill per output device. The core lists the
    /// devices when the screen opens; a skeleton shows until it answers.
    fn output_setting(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let selected = self.models.settings.output_device.as_ref();
        let default = pill(
            theme,
            ("output", 0usize),
            t::output_default(),
            selected.is_none(),
        )
        .tab_index(0)
        .aria_label(t::output_default())
        .on_click(cx.listener(|this, _, _, _| {
            this.change_settings(|settings| settings.output_device = None);
        }));
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space::S2)
            .pt(space::S1)
            .child(default);
        match &self.models.output_devices {
            None => {
                row = row.child(skeleton_line(theme));
            }
            Some(devices) if devices.is_empty() => {
                row = row.child(muted(theme, t::output_none())).child(
                    button(theme, "output-refresh", t::refresh(), ButtonKind::Secondary)
                        .aria_label(t::refresh())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.models.output_devices = None;
                            this.send(Command::ListOutputDevices);
                            cx.notify();
                        })),
                );
            }
            Some(devices) => {
                for (ix, device) in devices.iter().enumerate() {
                    let id = device.id.clone();
                    row = row.child(
                        pill(
                            theme,
                            ("output", ix + 1),
                            device.name.clone(),
                            selected == Some(&device.id),
                        )
                        .tab_index(0)
                        .aria_label(device.name.clone())
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.change_settings(|settings| {
                                settings.output_device = Some(id.clone());
                            });
                        })),
                    );
                }
            }
        }
        section(theme, t::output(), t::output_hint()).child(row)
    }

    /// Normalization, the equalizer preset and the volume boost (ADR 0022).
    fn sound_setting(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let settings = &self.models.settings;
        let (normalize, preset, boost) = (
            settings.normalize,
            settings.equalizer,
            settings.volume_boost,
        );
        let normalize_row = on_off_row(
            theme,
            "normalize",
            normalize,
            |settings, on| settings.normalize = on,
            cx,
        );
        let boost_row = on_off_row(
            theme,
            "boost",
            boost,
            |settings, on| settings.volume_boost = on,
            cx,
        );
        let presets = EqPreset::ALL.into_iter().enumerate().map(|(ix, value)| {
            let label = preset_label(value);
            pill(theme, ("equalizer", ix), label, preset == value)
                .tab_index(0)
                .aria_label(label)
                .on_click(cx.listener(move |this, _, _, _| {
                    this.change_settings(|settings| settings.equalizer = value);
                }))
        });
        section(theme, t::sound(), t::sound_hint())
            .child(sub_title(theme, t::normalize()))
            .child(muted(theme, t::normalize_hint()))
            .child(normalize_row)
            .child(sub_title(theme, t::equalizer()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(space::S2)
                    .pt(space::S1)
                    .children(presets),
            )
            .child(sub_title(theme, t::volume_boost()))
            .child(muted(theme, t::volume_boost_hint()))
            .child(boost_row)
    }

    /// "Show what I play on Discord", when this build can talk to Discord.
    fn discord_setting(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        if !self.discord.available() {
            return None;
        }
        let row = on_off_row(
            theme,
            "discord",
            self.models.settings.discord,
            |settings, on| settings.discord = on,
            cx,
        );
        Some(section(theme, d::setting(), d::setting_hint()).child(row))
    }

    /// The version, how the last check went, the button for the state, and
    /// the switch for automatic updates (ADR 0026).
    fn update_setting(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        let version = theme
            .text(div(), typography::BODY)
            .text_color(c.text)
            .child(u::version(env!("CARGO_PKG_VERSION")));
        let section = section(theme, u::title(), u::hint()).child(version);
        if !self.updates.available() {
            return section.child(muted(theme, u::unavailable()));
        }

        let state = &self.updates.state;
        let status = match state {
            UpdateState::Checking => skeleton_line(theme).into_any_element(),
            _ => {
                // The time zone lookup is only worth it when the line shows it.
                let checked_at = match state {
                    UpdateState::Reported(Progress::UpToDate) => {
                        self.updates.last_check.and_then(local_time)
                    }
                    _ => None,
                };
                theme
                    .text(div(), typography::BODY)
                    .text_color(c.text)
                    .child(status_text(state, checked_at))
                    .into_any_element()
            }
        };
        let check = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
            button(theme, id, label, ButtonKind::Secondary)
                .aria_label(label)
                .on_click(cx.listener(|this, _, _, cx| this.check_for_updates(true, cx)))
                .into_any_element()
        };
        let action = match state {
            UpdateState::Idle | UpdateState::Reported(Progress::UpToDate) => {
                Some(check("update-check", u::check_now(), cx))
            }
            UpdateState::Reported(Progress::Failed) => {
                Some(check("update-retry", app::try_again(), cx))
            }
            UpdateState::Reported(Progress::Available { .. }) => Some(
                button(
                    theme,
                    "update-download",
                    u::download(),
                    ButtonKind::Secondary,
                )
                .aria_label(u::download())
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.open_url(&sc_platform::update::releases_page());
                }))
                .into_any_element(),
            ),
            UpdateState::Reported(Progress::Ready(_)) => Some(
                button(theme, "update-restart", u::restart(), ButtonKind::Primary)
                    .aria_label(u::restart())
                    .on_click(cx.listener(|this, _, _, cx| this.restart_to_update(cx)))
                    .into_any_element(),
            ),
            UpdateState::Checking | UpdateState::Reported(Progress::Downloading { .. }) => None,
        };

        let auto_update = on_off_row(
            theme,
            "auto-update",
            self.models.settings.auto_update,
            |settings, on| settings.auto_update = on,
            cx,
        );
        section
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .pt(space::S1)
                    .child(status)
                    .children(action),
            )
            .child(sub_title(theme, u::automatic()))
            .child(auto_update)
    }

    /// The size of the artwork cache and "Clear cache".
    fn cache_setting(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        let size_label = match self.models.cache_size {
            Some(bytes) => theme
                .text(div(), typography::BODY)
                .text_color(c.text)
                .child(t::cache_size(megabytes(bytes)))
                .into_any_element(),
            None => skeleton_line(theme).into_any_element(),
        };
        section(theme, t::cache(), t::cache_hint()).child(
            div()
                .flex()
                .items_center()
                .gap(space::S4)
                .pt(space::S1)
                .child(size_label)
                .child(
                    button(
                        theme,
                        "clear-cache",
                        t::clear_cache(),
                        ButtonKind::Secondary,
                    )
                    .aria_label(t::clear_cache())
                    .on_click(cx.listener(|this, _, _, _| this.send(Command::ClearCache))),
                ),
        )
    }
}

/// The name of a preset, for its pill.
/// A one-line placeholder while a value loads.
fn skeleton_line(theme: &Theme) -> Div {
    div()
        .h(size::SKELETON_TITLE_HEIGHT)
        .w(size::SKELETON_ARTIST_WIDTH)
        .rounded(radius::S)
        .bg(theme.colors.surface_hover)
}

/// An On / Off pair of pills for a boolean setting; `set` writes the choice.
fn on_off_row(
    theme: &Theme,
    id: &'static str,
    current: bool,
    set: fn(&mut Settings, bool),
    cx: &mut Context<Shell>,
) -> Div {
    let choice = |ix: usize, label: &'static str, value: bool, cx: &mut Context<Shell>| {
        pill(theme, (id, ix), label, current == value)
            .tab_index(0)
            .aria_label(label)
            .on_click(cx.listener(move |this, _, _, _| {
                this.change_settings(|settings| set(settings, value));
            }))
    };
    div()
        .flex()
        .flex_wrap()
        .gap(space::S2)
        .pt(space::S1)
        .child(choice(0, app::on(), true, cx))
        .child(choice(1, app::off(), false, cx))
}

fn preset_label(preset: EqPreset) -> &'static str {
    match preset {
        EqPreset::Off => t::eq_off(),
        EqPreset::Bass => t::eq_bass(),
        EqPreset::Treble => t::eq_treble(),
        EqPreset::Vocal => t::eq_vocal(),
        EqPreset::Electronic => t::eq_electronic(),
    }
}

/// The name of one setting inside a section.
fn sub_title(theme: &Theme, title: &'static str) -> Div {
    div().pt(space::S2).child(
        theme
            .text(div(), typography::BODY)
            .text_color(theme.colors.text)
            .child(title),
    )
}

/// A titled group: the title, a muted hint, then whatever the caller adds.
fn section(theme: &Theme, title: &'static str, hint: &'static str) -> Div {
    div()
        .max_w(size::SEARCH_MAX_WIDTH)
        .flex()
        .flex_col()
        .gap(space::S2)
        .pb(space::S6)
        .child(
            theme
                .text(div(), typography::BODY)
                .text_color(theme.colors.text)
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(muted(theme, hint))
}

/// The keys the app answers to, with the key in the style of a key cap.
fn shortcut_list(theme: &Theme) -> Div {
    let c = theme.colors;
    let rows = shortcuts::help().map(|(keys, action)| {
        div()
            .flex()
            .items_center()
            .gap(space::S4)
            .child(
                div().w(size::SHORTCUT_KEY_WIDTH).flex().child(
                    theme
                        .text(div(), typography::MONO)
                        .px(space::S2)
                        .rounded(radius::S)
                        .border_1()
                        .border_color(c.line_strong)
                        .text_color(c.text_subtle)
                        .child(keys),
                ),
            )
            .child(
                theme
                    .text(div(), typography::BODY)
                    .text_color(c.text)
                    .child(action),
            )
    });
    div()
        .max_w(size::SEARCH_MAX_WIDTH)
        .flex()
        .flex_col()
        .gap(space::S2)
        .child(
            theme
                .text(div(), typography::BODY)
                .text_color(c.text)
                .font_weight(FontWeight::SEMIBOLD)
                .pb(space::S1)
                .child(t::shortcuts()),
        )
        .children(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn megabytes_has_one_decimal() {
        assert_eq!(megabytes(0), "0.0");
        assert_eq!(megabytes(1_572_864), "1.5");
        assert_eq!(megabytes(10 * 1_048_576), "10.0");
    }

    #[test]
    fn every_preset_has_a_label() {
        assert!(
            EqPreset::ALL
                .into_iter()
                .all(|p| !preset_label(p).is_empty())
        );
    }

    #[test]
    fn the_cache_size_is_a_text_with_its_unit() {
        assert_eq!(t::cache_size(megabytes(2_097_152)), "2.0 MB");
    }
}
