//! Your playlists in the shell (ADR 0014): the "Add to playlist" menu, the
//! dialogs to name or delete one, and the toasts after each change.

use std::path::PathBuf;

use cloudrs_ui::Theme;
use cloudrs_ui::components::{
    ButtonKind, Icon, ToastKind, button, dialog, icon, menu, menu_item, menu_separator, pill,
    tooltip,
};
use cloudrs_ui::search_field::SearchField;
use cloudrs_ui::tokens::{radius, size, space, typography};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, ObjectFit, PathPromptOptions, Pixels, Point, anchored, deferred,
    div, img, px,
};
use sc_core::{
    Command, ListItems, NewPlaylist, PlaylistChange, PlaylistId, PlaylistSummary, TrackId,
};

use super::Shell;
use crate::i18n::playlists as t;
use crate::models::ListId;
use crate::nav::Route;

/// The rest of a new playlist, beside its name: what the dialog fills.
pub struct PlaylistForm {
    pub description: Entity<SearchField>,
    pub genre: Entity<SearchField>,
    /// Comma-separated.
    pub tags: Entity<SearchField>,
    pub public: bool,
    /// The image picked for the cover.
    pub cover: Option<PathBuf>,
}

impl PlaylistForm {
    pub fn new(cx: &mut Context<Shell>) -> Self {
        let field = |placeholder: &'static str, glyph: Icon, cx: &mut Context<Shell>| {
            cx.new(|cx| SearchField::new(placeholder, "", cx).with_icon(glyph))
        };
        Self {
            description: field(t::description_placeholder(), Icon::Rename, cx),
            genre: field(t::genre_placeholder(), Icon::Queue, cx),
            tags: field(t::tags_placeholder(), Icon::Plus, cx),
            public: false,
            cover: None,
        }
    }

    /// Empty again, for the next playlist.
    fn reset(&mut self, cx: &mut Context<Shell>) {
        for field in [&self.description, &self.genre, &self.tags] {
            field.update(cx, |field, cx| field.reset(cx));
        }
        self.public = false;
        self.cover = None;
    }
}

/// A dialog over the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// Name a new playlist, with this track in it if any.
    NewPlaylist {
        track: Option<TrackId>,
    },
    Rename(PlaylistId),
    /// Write the description of one of the person's playlists.
    Describe(PlaylistId),
    /// Confirm deleting a playlist (its title, to name it).
    Delete(PlaylistId, String),
}

impl Shell {
    /// The person's own playlists (the library also holds liked ones).
    pub(crate) fn own_playlists(&self) -> Vec<PlaylistSummary> {
        let Some(me) = self.models.account.as_ref().map(|me| me.id) else {
            return Vec::new();
        };
        match self.models.lists.get(&ListId::Library).map(|l| &l.items) {
            Some(ListItems::Playlists(playlists)) => playlists
                .iter()
                .filter(|p| p.owner_id == Some(me) && !p.is_album)
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Whether this playlist is the signed-in person's.
    pub(crate) fn owns_playlist(&self, id: PlaylistId) -> bool {
        let Some(me) = self.models.account.as_ref().map(|me| me.id) else {
            return false;
        };
        matches!(
            self.models.playlists.get(&id),
            Some(crate::models::Page::Ready(page)) if page.owner_id == Some(me)
        )
    }

    pub(crate) fn open_playlist_menu(
        &mut self,
        track: TrackId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.models.lists.contains_key(&ListId::Library) {
            self.dispatch(crate::intent::UiIntent::OpenList(ListId::Library), cx);
        }
        self.playlist_menu = Some((track, at));
        cx.notify();
    }

    pub(crate) fn open_dialog(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        let name = match &dialog {
            Dialog::Rename(id) => self.playlist_title(*id).unwrap_or_default(),
            _ => String::new(),
        };
        self.name_field
            .update(cx, |field, cx| field.set_value(&name, cx));
        if matches!(dialog, Dialog::NewPlaylist { .. }) {
            self.form.reset(cx);
        }
        if let Dialog::Describe(id) = &dialog {
            let text = self.playlist_description(*id);
            self.form
                .description
                .update(cx, |field, cx| field.set_value(&text, cx));
        }
        self.playlist_menu = None;
        self.dialog = Some(dialog);
        cx.notify();
    }

    fn playlist_title(&self, id: PlaylistId) -> Option<String> {
        match self.models.playlists.get(&id) {
            Some(crate::models::Page::Ready(page)) => Some(page.title.clone()),
            _ => None,
        }
    }

    fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        cx.notify();
    }

    /// The "Add to playlist" menu, floating where it was opened.
    pub(crate) fn playlist_menu_view(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (track, at) = self.playlist_menu?;
        let mut list = menu(theme, "playlist-menu")
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.playlist_menu = None;
                cx.notify();
            }))
            .child(
                menu_item(theme, "menu-new-playlist", Icon::Plus, t::new_playlist())
                    .aria_label(t::new_playlist())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_dialog(Dialog::NewPlaylist { track: Some(track) }, cx);
                    })),
            );
        let own = self.own_playlists();
        if !own.is_empty() {
            list = list.child(menu_separator(theme));
        }
        for (ix, playlist) in own.into_iter().enumerate() {
            let id = playlist.id;
            list = list.child(
                menu_item(
                    theme,
                    ("menu-playlist", ix),
                    Icon::Queue,
                    playlist.title.clone(),
                )
                .aria_label(t::add_to(&playlist.title))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.playlist_menu = None;
                    this.send(Command::AddToPlaylist {
                        playlist: id,
                        track,
                        at: None,
                    });
                    cx.notify();
                })),
            );
        }
        Some(
            deferred(anchored().position(at).snap_to_window().child(list))
                .with_priority(1)
                .into_any_element(),
        )
    }

    /// The dialog showing, over the whole window.
    pub(crate) fn dialog_view(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dismiss = cx.listener(|this, _, _, cx| this.close_dialog(cx));
        let cancel = button(theme, "dialog-cancel", t::cancel(), ButtonKind::Secondary)
            .aria_label(t::cancel())
            .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx)))
            .into_any_element();
        let view = match self.dialog.clone()? {
            Dialog::NewPlaylist { track } => {
                let create = button(theme, "dialog-create", t::create(), ButtonKind::Primary)
                    .aria_label(t::create())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let title = this.name_field.read(cx).value().trim().to_owned();
                        if title.is_empty() {
                            return;
                        }
                        let read = |field: &Entity<SearchField>, cx: &Context<Shell>| {
                            field.read(cx).value().trim().to_owned()
                        };
                        let new = NewPlaylist {
                            title,
                            description: read(&this.form.description, cx),
                            public: this.form.public,
                            genre: read(&this.form.genre, cx),
                            tags: read(&this.form.tags, cx)
                                .split(',')
                                .map(str::to_owned)
                                .collect(),
                            cover: this.form.cover.clone(),
                            track,
                        };
                        this.send(Command::CreatePlaylist(new));
                        this.close_dialog(cx);
                    }))
                    .into_any_element();
                dialog(
                    theme,
                    "dialog-new",
                    t::new_playlist_title(),
                    vec![self.new_playlist_form(theme, cx)],
                    vec![cancel, create],
                    dismiss,
                )
            }
            Dialog::Rename(id) => {
                let save = button(theme, "dialog-save", t::save(), ButtonKind::Primary)
                    .aria_label(t::save())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let title = this.name_field.read(cx).value().trim().to_owned();
                        if title.is_empty() {
                            return;
                        }
                        this.send(Command::RenamePlaylist {
                            playlist: id,
                            title,
                        });
                        this.close_dialog(cx);
                    }))
                    .into_any_element();
                dialog(
                    theme,
                    "dialog-rename",
                    t::rename_title(),
                    vec![self.name_field.clone().into_any_element()],
                    vec![cancel, save],
                    dismiss,
                )
            }
            Dialog::Describe(id) => {
                let save = button(theme, "dialog-save", t::save(), ButtonKind::Primary)
                    .aria_label(t::save())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let description = this.form.description.read(cx).value().to_owned();
                        this.send(Command::SetPlaylistDescription {
                            playlist: id,
                            description,
                        });
                        this.close_dialog(cx);
                    }))
                    .into_any_element();
                dialog(
                    theme,
                    "dialog-describe",
                    t::description_title(),
                    vec![self.form.description.clone().into_any_element()],
                    vec![cancel, save],
                    dismiss,
                )
            }
            Dialog::Delete(id, title) => {
                let delete = button(theme, "dialog-delete", t::delete(), ButtonKind::Primary)
                    .aria_label(t::delete())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send(Command::DeletePlaylist(id));
                        this.close_dialog(cx);
                    }))
                    .into_any_element();
                dialog(
                    theme,
                    "dialog-delete",
                    t::delete_title(&title),
                    vec![hint(theme, t::delete_hint())],
                    vec![cancel, delete],
                    dismiss,
                )
            }
        };
        Some(view.into_any_element())
    }

    /// A playlist was saved: say so, with "Undo" after a removal. A deleted
    /// playlist whose page shows sends the person back.
    pub(crate) fn playlist_saved(
        &mut self,
        playlist: &PlaylistSummary,
        change: PlaylistChange,
        cx: &mut Context<Self>,
    ) {
        let name = &playlist.title;
        let text = match change {
            PlaylistChange::Created => t::created(name),
            PlaylistChange::Added(_) => t::added(name),
            PlaylistChange::AlreadyThere(_) => t::already_there(name),
            PlaylistChange::Removed => t::removed(name),
            PlaylistChange::Moved => return,
            PlaylistChange::Renamed => t::renamed(name),
            PlaylistChange::Privacy { public: true } => t::now_public(name),
            PlaylistChange::Privacy { public: false } => t::now_private(name),
            PlaylistChange::Deleted => t::deleted(name),
            PlaylistChange::Described => t::described(name),
            PlaylistChange::Cover => t::new_cover(name),
        };
        let undo = match change {
            PlaylistChange::Removed => self.pending_undo.take(),
            _ => None,
        };
        if change == PlaylistChange::Deleted
            && *self.router.current() == Route::Playlist(playlist.id)
        {
            self.go_back(cx);
        }
        self.show_toast_undo(ToastKind::Info, text, undo, cx);
    }
}

impl Shell {
    /// The cover picker beside the name and description, then genre and
    /// tags, then public or private.
    fn new_playlist_form(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        let cover = div()
            .id("dialog-cover")
            .flex_none()
            .size(size::COVER_PICKER)
            .rounded(radius::L)
            .overflow_hidden()
            .border_1()
            .border_color(c.line_strong)
            .bg(c.surface_raised)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::S1)
            .cursor_pointer()
            .tab_index(0)
            .focus_visible(move |s| s.border_color(c.accent))
            .hover(move |s| s.border_color(c.accent))
            .aria_label(t::choose_cover())
            .tooltip(tooltip(t::choose_cover()))
            .map(|cover| match &self.form.cover {
                Some(path) => cover.child(
                    img(path.clone())
                        .size_full()
                        .rounded(radius::L)
                        .object_fit(ObjectFit::Cover),
                ),
                None => cover
                    .child(icon(Icon::Plus, size::ICON_M, c.text_muted))
                    .child(
                        theme
                            .text(div(), typography::LABEL)
                            .text_color(c.text_muted)
                            .child(t::cover_label()),
                    ),
            })
            .on_click(cx.listener(|this, _, _, cx| this.pick_cover(cx)));
        let public = self.form.public;
        let privacy = |ix: usize, label: &'static str, on: bool| {
            pill(theme, ("dialog-privacy", ix), label, on)
                .tab_index(0)
                .aria_label(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.form.public = ix == 1;
                    cx.notify();
                }))
        };
        div()
            .flex()
            .flex_col()
            .gap(space::S3)
            .child(
                div().flex().gap(space::S3).child(cover).child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .child(self.name_field.clone())
                        .child(self.form.description.clone()),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap(space::S2)
                    .child(div().flex_1().child(self.form.genre.clone()))
                    .child(div().flex_1().child(self.form.tags.clone())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .child(privacy(0, t::private_label(), !public))
                    .child(privacy(1, t::public_label(), public)),
            )
            .child(hint(
                theme,
                if public {
                    t::public_hint()
                } else {
                    t::private_hint()
                },
            ))
            .into_any_element()
    }

    /// The system's file picker, for the cover image.
    fn pick_cover(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t::choose_cover().into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.form.cover = Some(path);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}

fn hint(theme: &Theme, text: &'static str) -> AnyElement {
    theme
        .text(div(), typography::BODY_MUTED)
        .text_color(theme.colors.text_muted)
        .child(text)
        .into_any_element()
}

impl Shell {
    fn playlist_description(&self, id: PlaylistId) -> String {
        match self.models.playlists.get(&id) {
            Some(crate::models::Page::Ready(page)) => page.description.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Picks an image and sends it as the cover of one of the person's playlists.
    pub(crate) fn change_playlist_cover(&mut self, id: PlaylistId, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t::choose_cover().into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(cover) = paths.into_iter().next()
            {
                this.update(cx, |this, _| {
                    this.send(Command::SetPlaylistCover {
                        playlist: id,
                        cover,
                    });
                })
                .ok();
            }
        })
        .detach();
    }
}
