//! The Settings screen (ADR 0017), in groups, each a titled card of rows
//! (name and hint on the left, the control on the right): appearance (theme,
//! language), sound (the audio output, ADR 0020, normalization, equalizer and
//! volume boost, ADR 0022), privacy and storage (Discord, the artwork cache),
//! updates (ADR 0026) and the keyboard shortcuts. Every change goes to the
//! core as a whole `Settings` and takes effect when the core echoes it.

use cloudrs_ui::Theme;
use cloudrs_ui::components::{ButtonKind, button, pill};
use cloudrs_ui::scrollbar::scrollbar;
use cloudrs_ui::tokens::{radius, size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, SharedString, div, px};
use sc_core::{Command, EqPreset, Settings, ThemeChoice};
use sc_platform::update::Progress;

use super::account::muted;
use crate::i18n::{self, Language, app, discord as d, settings as t, update as u};
use crate::shell::updates::{UpdateState, local_time, status_text};
use crate::shell::{Shell, shortcuts};

/// Megabytes with one decimal, for the cache size.
fn megabytes(bytes: u64) -> String {
    i18n::number::one_decimal(bytes as f64 / 1_048_576.0)
}

impl Shell {
    pub(crate) fn settings_screen(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("settings")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.page_scroll)
                    .child(
                        div()
                            .w_full()
                            .max_w(size::SETTINGS_MAX_WIDTH)
                            .flex()
                            .flex_col()
                            .gap(space::S7)
                            .px(space::S5)
                            .pt(space::S2)
                            .pb(space::S8)
                            .child(
                                theme
                                    .text(div(), typography::DISPLAY_L)
                                    .text_color(c.text)
                                    .child(t::title()),
                            )
                            .child(self.appearance_group(theme, cx))
                            .child(self.sound_group(theme, cx))
                            .child(self.privacy_group(theme, cx))
                            .child(self.update_group(theme, cx))
                            .child(shortcut_group(theme)),
                    ),
            )
            .child(scrollbar(
                "settings-scrollbar",
                &self.page_scroll,
                i18n::app::scrollbar(),
            ))
            .into_any_element()
    }

    /// Theme (System, Dark or Light) and language, one pill per language in
    /// its own name (ADR 0025).
    fn appearance_group(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let selected = self.models.settings.theme;
        let choice = |ix: usize, label: &'static str, value: ThemeChoice| {
            pill(theme, ("theme", ix), label, selected == value)
                .tab_index(0)
                .aria_label(t::choice(t::theme(), label))
                .on_click(cx.listener(move |this, _, _, _| {
                    this.change_settings(|settings| settings.theme = value);
                }))
        };
        let themes = div()
            .flex()
            .gap(space::S2)
            .child(choice(0, t::theme_system(), ThemeChoice::System))
            .child(choice(1, t::theme_dark(), ThemeChoice::Dark))
            .child(choice(2, t::theme_light(), ThemeChoice::Light));

        let selected = self.models.settings.language;
        let mut languages = div().flex().flex_wrap().gap(space::S2);
        for (ix, language) in Language::ALL.into_iter().enumerate() {
            let name = i18n::language_name(language);
            languages = languages.child(
                pill(theme, ("language", ix), name, selected == language)
                    .tab_index(0)
                    .aria_label(t::choice(t::language(), name))
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.change_settings(|settings| settings.language = language);
                    })),
            );
        }

        group(
            theme,
            t::appearance(),
            vec![
                row(
                    setting_label(theme, t::theme(), muted(theme, t::theme_hint())),
                    themes,
                ),
                row(
                    setting_label(theme, t::language(), muted(theme, t::language_hint())),
                    languages,
                ),
            ],
        )
    }

    /// The output device, normalization, the equalizer preset and the volume
    /// boost.
    fn sound_group(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let settings = &self.models.settings;
        let (normalize, preset, boost) = (
            settings.normalize,
            settings.equalizer,
            settings.volume_boost,
        );
        let normalize_row = on_off(
            theme,
            "normalize",
            t::normalize(),
            normalize,
            |settings, on| settings.normalize = on,
            cx,
        );
        let boost_row = on_off(
            theme,
            "boost",
            t::volume_boost(),
            boost,
            |settings, on| settings.volume_boost = on,
            cx,
        );
        let presets = EqPreset::ALL.into_iter().enumerate().map(|(ix, value)| {
            let label = preset_label(value);
            pill(theme, ("equalizer", ix), label, preset == value)
                .tab_index(0)
                .aria_label(t::choice(t::equalizer(), label))
                .on_click(cx.listener(move |this, _, _, _| {
                    this.change_settings(|settings| settings.equalizer = value);
                }))
        });
        let presets = div().flex().flex_wrap().gap(space::S2).children(presets);

        group(
            theme,
            t::sound(),
            vec![
                stacked_row(
                    setting_label(theme, t::output(), muted(theme, t::output_hint())),
                    self.output_choices(theme, cx),
                ),
                row(
                    setting_label(theme, t::normalize(), muted(theme, t::normalize_hint())),
                    normalize_row,
                ),
                stacked_row(
                    setting_label(theme, t::equalizer(), muted(theme, t::equalizer_hint())),
                    presets,
                ),
                row(
                    setting_label(
                        theme,
                        t::volume_boost(),
                        muted(theme, t::volume_boost_hint()),
                    ),
                    boost_row,
                ),
            ],
        )
    }

    /// System default, then one pill per output device. The core lists the
    /// devices when the screen opens; a skeleton shows until it answers.
    fn output_choices(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let selected = self.models.settings.output_device.as_ref();
        let default = pill(
            theme,
            ("output", 0usize),
            t::output_default(),
            selected.is_none(),
        )
        .tab_index(0)
        .aria_label(t::choice(t::output(), t::output_default()))
        .on_click(cx.listener(|this, _, _, _| {
            this.change_settings(|settings| settings.output_device = None);
        }));
        let mut choices = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space::S2)
            .child(default);
        match &self.models.output_devices {
            None => {
                choices = choices.child(skeleton_line(theme));
            }
            Some(devices) if devices.is_empty() => {
                choices = choices.child(muted(theme, t::output_none())).child(
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
                    choices = choices.child(
                        pill(
                            theme,
                            ("output", ix + 1),
                            device.name.clone(),
                            selected == Some(&device.id),
                        )
                        .tab_index(0)
                        .aria_label(t::choice(t::output(), &device.name))
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.change_settings(|settings| {
                                settings.output_device = Some(id.clone());
                            });
                        })),
                    );
                }
            }
        }
        choices
    }

    /// "Show what I play on Discord", when this build can talk to Discord,
    /// and the artwork cache.
    fn privacy_group(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        let mut rows = Vec::new();
        if self.discord.available() {
            let discord = on_off(
                theme,
                "discord",
                d::setting(),
                self.models.settings.discord,
                |settings, on| settings.discord = on,
                cx,
            );
            rows.push(row(
                setting_label(theme, d::setting(), muted(theme, d::setting_hint())),
                discord,
            ));
        }

        let size_label = match self.models.cache_size {
            Some(bytes) => theme
                .text(div(), typography::BODY_MUTED)
                .text_color(c.text_muted)
                .child(t::cache_size(megabytes(bytes)))
                .into_any_element(),
            None => skeleton_line(theme).into_any_element(),
        };
        let cache = div()
            .flex()
            .items_center()
            .gap(space::S3)
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
            );
        rows.push(row(
            setting_label(theme, t::cache(), muted(theme, t::cache_hint())),
            cache,
        ));
        group(theme, t::privacy_storage(), rows)
    }

    /// The version, how the last check went, the button for the state, and
    /// whether updates install on their own (ADR 0026).
    fn update_group(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = theme.colors;
        let version = u::version(env!("CARGO_PKG_VERSION"));
        if !self.updates.available() {
            return group(
                theme,
                u::title(),
                vec![row(
                    setting_label(theme, version, muted(theme, u::unavailable())),
                    div(),
                )],
            );
        }

        let state = &self.updates.state;
        let status = match state {
            UpdateState::Checking => skeleton_line(theme),
            _ => {
                // The time zone lookup is only worth it when the line shows it.
                let checked_at = match state {
                    UpdateState::Reported(Progress::UpToDate) => {
                        self.updates.last_check.and_then(local_time)
                    }
                    _ => None,
                };
                theme
                    .text(div(), typography::BODY_MUTED)
                    .text_color(c.text_muted)
                    .child(status_text(state, checked_at))
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

        let auto_update = on_off(
            theme,
            "auto-update",
            u::automatic(),
            self.models.settings.auto_update,
            |settings, on| settings.auto_update = on,
            cx,
        );
        group(
            theme,
            u::title(),
            vec![
                row(
                    setting_label(theme, version, status),
                    div().children(action),
                ),
                row(
                    setting_label(theme, u::automatic(), muted(theme, u::hint())),
                    auto_update,
                ),
            ],
        )
    }
}

/// A one-line placeholder while a value loads.
fn skeleton_line(theme: &Theme) -> Div {
    div()
        .h(size::SKELETON_TITLE_HEIGHT)
        .w(size::SKELETON_ARTIST_WIDTH)
        .rounded(radius::S)
        .bg(theme.colors.surface_hover)
}

/// An On / Off pair of pills for a boolean setting; `set` writes the choice.
/// `name` is the setting, so the pills read "Normalize volume: On".
fn on_off(
    theme: &Theme,
    id: &'static str,
    name: &'static str,
    current: bool,
    set: fn(&mut Settings, bool),
    cx: &mut Context<Shell>,
) -> Div {
    let choice = |ix: usize, label: &'static str, value: bool, cx: &mut Context<Shell>| {
        pill(theme, (id, ix), label, current == value)
            .tab_index(0)
            .aria_label(t::choice(name, label))
            .on_click(cx.listener(move |this, _, _, _| {
                this.change_settings(|settings| set(settings, value));
            }))
    };
    div()
        .flex()
        .gap(space::S2)
        .child(choice(0, app::on(), true, cx))
        .child(choice(1, app::off(), false, cx))
}

/// The name of a preset, for its pill.
fn preset_label(preset: EqPreset) -> &'static str {
    match preset {
        EqPreset::Off => t::eq_off(),
        EqPreset::Bass => t::eq_bass(),
        EqPreset::Treble => t::eq_treble(),
        EqPreset::Vocal => t::eq_vocal(),
        EqPreset::Electronic => t::eq_electronic(),
        EqPreset::Techno => t::eq_techno(),
        EqPreset::HardTechno => t::eq_hard_techno(),
        EqPreset::Hardstyle => t::eq_hardstyle(),
        EqPreset::Hardcore => t::eq_hardcore(),
    }
}

/// The name of a setting over its hint (any element, usually muted text).
fn setting_label(theme: &Theme, title: impl Into<SharedString>, hint: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(space::S1)
        .child(
            theme
                .text(div(), typography::BODY)
                .text_color(theme.colors.text)
                .child(title.into()),
        )
        .child(hint)
}

/// A setting: its label on the left, its control on the right.
fn row(label: Div, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(space::S6)
        .px(space::S4)
        .py(space::S3)
        .child(label)
        .child(div().flex_none().child(control))
}

/// A setting with a wide choice: the label on top, the control under it.
fn stacked_row(label: Div, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::S3)
        .px(space::S4)
        .py(space::S3)
        .child(label)
        .child(control)
}

/// A titled group: the title over a card whose rows a line separates.
fn group(theme: &Theme, title: &'static str, rows: Vec<Div>) -> Div {
    let c = theme.colors;
    let card = rows.into_iter().enumerate().fold(
        div()
            .flex()
            .flex_col()
            .rounded(radius::L)
            .border_1()
            .border_color(c.line)
            .bg(c.surface),
        |card, (ix, row)| {
            card.child(if ix > 0 {
                row.border_t_1().border_color(c.line)
            } else {
                row
            })
        },
    );
    div()
        .flex()
        .flex_col()
        .gap(space::S3)
        .child(
            theme
                .text(div(), typography::TITLE)
                .text_color(c.text)
                .child(title),
        )
        .child(card)
}

/// The keys the app answers to: the action on the left, the key on the right
/// in the style of a key cap.
fn shortcut_group(theme: &Theme) -> Div {
    let c = theme.colors;
    let rows = shortcuts::help()
        .into_iter()
        .map(|(keys, action)| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(space::S4)
                .px(space::S4)
                .py(space::S2)
                .child(
                    theme
                        .text(div(), typography::BODY)
                        .text_color(c.text)
                        .child(action),
                )
                .child(
                    theme
                        .text(div(), typography::MONO)
                        .flex_none()
                        .px(space::S2)
                        .rounded(radius::S)
                        .border_1()
                        .border_color(c.line_strong)
                        .text_color(c.text_subtle)
                        .child(keys),
                )
        })
        .collect();
    group(theme, t::shortcuts(), rows)
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

    #[test]
    fn a_choice_names_its_setting() {
        assert_eq!(t::choice("Theme", "Dark"), "Theme: Dark");
    }
}
