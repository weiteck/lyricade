use relm4::gtk::prelude::{CheckButtonExt, WidgetExt};
use relm4::prelude::*;
use relm4::typed_view::{TypedListItem, column::*};
use tracing::{debug, error};

use crate::track::Track;
use crate::util::{self, now};

pub struct TracksTableModel {
    table: TypedColumnView<Track, gtk::MultiSelection>,
}

#[derive(Debug)]
pub enum TracksTableMsg {
    ClearAndAppend(Vec<Track>),
    Update(Track),
    Filter(Option<String>),
}

#[relm4::component(pub)]
impl SimpleComponent for TracksTableModel {
    type Init = ();
    type Input = TracksTableMsg;
    type Output = ();

    view! {
        gtk::ScrolledWindow {
          set_hexpand: true,

          #[local_ref]
          tracks_table_view -> gtk::ColumnView {
            set_expand: true,
            set_show_column_separators: true,
          },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut table = TypedColumnView::<Track, gtk::MultiSelection>::new();

        // Append each column
        table.append_column::<TracksTableColumnArtist>();
        table.append_column::<TracksTableColumnAlbum>();
        table.append_column::<TracksTableColumnTrack>();
        table.append_column::<TracksTableColumnLyrics>();
        table.append_column::<TracksTableColumnLyricsSync>();
        table.append_column::<TracksTableColumnSidecar>();
        table.append_column::<TracksTableColumnChecked>();
        table.append_column::<TracksTableColumnModified>();

        if cfg!(debug_assertions) {
            table.append_column::<TracksTableColumnDebugTimestamp>();
        }

        let model = TracksTableModel { table };

        // Ref of the view to use in `view!` macro
        let tracks_table_view = &model.table.view;

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            TracksTableMsg::ClearAndAppend(tracks) => {
                self.table.clear();
                self.table.extend_from_iter(tracks);
            }

            TracksTableMsg::Filter(query) => {
                if let Some(query) = query {
                    self.table.clear_filters();

                    for token in query.to_lowercase().split_whitespace().map(String::from) {
                        self.table.add_filter(move |track| {
                            track.artist_name.to_lowercase().contains(&token)
                                || track.album_name.to_lowercase().contains(&token)
                                || track.track_name.to_lowercase().contains(&token)
                        });
                    }
                } else {
                    self.table.clear_filters();
                }
            }

            TracksTableMsg::Update(track) => {
                if let Some(row) = self
                    .table
                    .find(|row| row.id == track.id)
                    .and_then(|idx| self.table.get(idx))
                {
                    let mut row = row.borrow_mut();

                    debug!("Updating {row} in TracksTable");

                    row.lyrics = track.lyrics;
                    row.lyrics_synchronised = track.lyrics_synchronised;
                    row.lyrics_sidecar_lrc_file = track.lyrics_sidecar_lrc_file;
                    row.lyrics_sidecar_txt_file = track.lyrics_sidecar_txt_file;
                    row.last_api_check_at = track.last_api_check_at;
                    row.file_modified_at = track.file_modified_at;
                } else {
                    error!("Could not find {track} in TracksTable");
                }
            }
        }
    }
}

// Column Models ///////////////////////////////////////////////////////////////
struct TracksTableColumnDebugTimestamp;
impl LabelColumn for TracksTableColumnDebugTimestamp {
    type Item = Track;
    type Value = String;

    const COLUMN_NAME: &'static str = "ROW CREATED";
    const ENABLE_SORT: bool = true;

    fn get_cell_value(_item: &Self::Item) -> Self::Value {
        util::ndt_utc_to_local_dt(now())
            .format("%T%.3f")
            .to_string()
    }
}

struct TracksTableColumnArtist;
impl RelmColumn for TracksTableColumnArtist {
    type Item = Track;
    type Root = gtk::Label;
    type Widgets = ();

    const COLUMN_NAME: &'static str = "Artist";
    const ENABLE_RESIZE: bool = true;
    const ENABLE_EXPAND: bool = true;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let label = gtk::Label::new(None);
        label.set_align(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        (label, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        root.set_label(&item.artist_name);
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| a.artist_name.cmp(&b.artist_name)))
    }
}

struct TracksTableColumnAlbum;
impl RelmColumn for TracksTableColumnAlbum {
    type Item = Track;
    type Root = gtk::Label;
    type Widgets = ();

    const COLUMN_NAME: &'static str = "Album";
    const ENABLE_RESIZE: bool = true;
    const ENABLE_EXPAND: bool = true;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let label = gtk::Label::new(None);
        label.set_align(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        (label, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        root.set_label(&item.album_name);
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| a.album_name.cmp(&b.album_name)))
    }
}

struct TracksTableColumnTrack;
impl RelmColumn for TracksTableColumnTrack {
    type Item = Track;
    type Root = gtk::Label;
    type Widgets = ();

    const COLUMN_NAME: &'static str = "Track";
    const ENABLE_RESIZE: bool = true;
    const ENABLE_EXPAND: bool = true;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let label = gtk::Label::new(None);
        label.set_align(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        (label, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        root.set_label(&item.track_name);
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| a.track_name.cmp(&b.track_name)))
    }
}

struct TracksTableColumnLyrics;
impl RelmColumn for TracksTableColumnLyrics {
    type Root = gtk::CheckButton;
    type Widgets = ();
    type Item = Track;

    const COLUMN_NAME: &'static str = "Lyrics";

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let checkbutton = gtk::CheckButton::new();
        checkbutton.set_can_focus(false);
        checkbutton.set_can_target(false);
        (checkbutton, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        root.set_active(item.lyrics.is_some());
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| a.lyrics.is_some().cmp(&b.lyrics.is_some())))
    }
}

struct TracksTableColumnLyricsSync;
impl RelmColumn for TracksTableColumnLyricsSync {
    type Root = gtk::CheckButton;
    type Widgets = ();
    type Item = Track;

    const COLUMN_NAME: &'static str = "Sync";

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let checkbutton = gtk::CheckButton::new();
        checkbutton.set_can_focus(false);
        checkbutton.set_can_target(false);
        (checkbutton, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        root.set_active(item.lyrics_synchronised);
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| {
            a.lyrics_synchronised.cmp(&b.lyrics_synchronised)
        }))
    }
}

struct TracksTableColumnSidecar;
impl RelmColumn for TracksTableColumnSidecar {
    type Root = gtk::Label;
    type Widgets = ();
    type Item = Track;

    const COLUMN_NAME: &'static str = "Sidecar";

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let label = gtk::Label::new(None);
        label.set_align(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        (label, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        match (&item.lyrics_sidecar_lrc_file, &item.lyrics_sidecar_txt_file) {
            (Some(_), _) => root.set_label("LRC"),
            (None, Some(_)) => root.set_label("TXT"),
            _ => (),
        }
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| {
            a.lyrics_sidecar_lrc_file
                .is_some()
                .cmp(&b.lyrics_sidecar_lrc_file.is_some())
                .then_with(|| {
                    a.lyrics_sidecar_txt_file
                        .is_some()
                        .cmp(&b.lyrics_sidecar_txt_file.is_some())
                })
        }))
    }
}

struct TracksTableColumnChecked;
impl RelmColumn for TracksTableColumnChecked {
    type Item = Track;
    type Root = gtk::Label;
    type Widgets = ();

    const COLUMN_NAME: &'static str = "Checked";

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let label = gtk::Label::new(None);
        label.set_align(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_width_chars(20);
        (label, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        let s = item
            .last_api_check_at
            .map(|ndt| util::ndt_utc_to_local_dt(ndt).format("%F %T").to_string())
            .unwrap_or_else(|| "Never".into());
        root.set_label(&s);
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| {
            a.last_api_check_at.cmp(&b.last_api_check_at)
        }))
    }
}

struct TracksTableColumnModified;
impl RelmColumn for TracksTableColumnModified {
    type Item = Track;
    type Root = gtk::Label;
    type Widgets = ();

    const COLUMN_NAME: &'static str = "Modified";

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let label = gtk::Label::new(None);
        label.set_align(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_width_chars(20);
        (label, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
        let s = util::ndt_utc_to_local_dt(item.file_modified_at)
            .format("%F %T")
            .to_string();
        root.set_label(&s);
    }

    fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
        Some(Box::new(|a, b| a.file_modified_at.cmp(&b.file_modified_at)))
    }
}
