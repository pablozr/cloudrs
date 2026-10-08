//! The Settings screen (ADR 0017): theme, the audio output (ADR 0020), Discord,
//! the artwork cache and the keyboard shortcuts. Every change goes to the core as a whole `Settings`
//! and takes effect when the core echoes it.

use cloudrs_ui::Theme;
use cloudrs_ui::components::{ButtonKind, button, pill};
use cloudrs_ui::tokens::{radius, size, space, typography};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, div};
use sc_core::{Command, ThemeChoice};

use super::account::muted;
use crate::i18n::{discord as d, settings as t};
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
            .children(self.discord_setting(theme, cx))
            // The language picker appears with a second language (ADR 0017).
            .child(self.cache_setting(theme, cx))
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
        let c = theme.colors;
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
                row = row.child(
                    div()
                        .h(size::SKELETON_TITLE_HEIGHT)
                        .w(size::SKELETON_ARTIST_WIDTH)
                        .rounded(radius::S)
                        .bg(c.surface_hover),
                );
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

    /// "Show what I play on Discord", when this build can talk to Discord.
    fn discord_setting(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        if !self.discord.available() {
            return None;
        }
        let on = self.models.settings.discord;
        let choice = |ix: usize, label: &'static str, selected: bool| {
            pill(theme, ("discord", ix), label, selected)
                .tab_index(0)
                .aria_label(label)
                .on_click(cx.listener(move |this, _, _, _| {
                    this.change_settings(|settings| settings.discord = ix == 0);
                }))
        };
        Some(
            section(theme, d::setting(), d::setting_hint()).child(
                div()
                    .flex()
                    .gap(space::S2)
                    .pt(space::S1)
                    .child(choice(0, d::on(), on))
                    .child(choice(1, d::off(), !on)),
            ),
        )
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
            None => div()
                .h(size::SKELETON_TITLE_HEIGHT)
                .w(size::SKELETON_ARTIST_WIDTH)
                .rounded(radius::S)
                .bg(c.surface_hover)
                .into_any_element(),
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
    fn the_cache_size_is_a_text_with_its_unit() {
        assert_eq!(t::cache_size(megabytes(2_097_152)), "2.0 MB");
    }
}
