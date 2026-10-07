//! Your playlists in the shell (ADR 0014): the "Add to playlist" menu, the
//! dialogs to name or delete one, and the toasts after each change.

use cloudrs_ui::Theme;
use cloudrs_ui::components::{
    ButtonKind, Icon, ToastKind, button, dialog, menu, menu_item, menu_separator,
};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Pixels, Point, anchored, deferred, div};
use sc_core::{Command, ListItems, PlaylistChange, PlaylistId, PlaylistSummary, TrackId};

use super::Shell;
use crate::i18n::playlists as t;
use crate::models::ListId;
use crate::nav::Route;

/// A dialog over the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// Name a new playlist, with this track in it if any.
    NewPlaylist {
        track: Option<TrackId>,
    },
    Rename(PlaylistId),
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
                        this.send(Command::CreatePlaylist { title, track });
                        this.close_dialog(cx);
                    }))
                    .into_any_element();
                dialog(
                    theme,
                    "dialog-new",
                    t::new_playlist_title(),
                    vec![
                        self.name_field.clone().into_any_element(),
                        hint(theme, t::private_hint()),
                    ],
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

fn hint(theme: &Theme, text: &'static str) -> AnyElement {
    theme
        .text(div(), cloudrs_ui::tokens::typography::BODY_MUTED)
        .text_color(theme.colors.text_muted)
        .child(text)
        .into_any_element()
}
