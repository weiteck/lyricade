use std::collections::HashSet;

use relm4::gtk::prelude::{SelectionModelExt, WidgetExt};
use relm4::gtk::{Bitset, BitsetIter, EventControllerKey};
use relm4::prelude::*;
use relm4::typed_view::column::*;
use tracing::{debug, trace};

use crate::SETTINGS;
use crate::track::Track;
use crate::util::{self};

pub struct TracksTableModel {
  table: TypedColumnView<Track, gtk::MultiSelection>,
  preset_filters_len: usize,
  total_rows: u32,
  rows_visible: bool,
}

#[derive(Debug)]
pub enum TracksTableMsg {
  ClearAndAppend(Vec<Track>),
  Update(Track),
  Filter(Option<String>),
  SetFilter((TracksTableFilter, bool)),
  ClearFilters,
  HandleRowActivated,
  HandleRowSelection(Bitset),
  ClearSelection,
}

#[derive(Debug)]
pub enum TracksTableOutput {
  TrackIdsSelected(HashSet<i32>),
  RowActivated,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum TracksTableFilter {
  NeverChecked = 0,
  NoLyrics = 1,
  NoLyricsTag = 2,
  NotInstrumental = 3,
  NotSync = 4,
  Lrc = 5,
  Txt = 6,
  EitherLrcOrTxt = 7,
}

#[relm4::component(pub)]
impl SimpleComponent for TracksTableModel {
  type Init = ();
  type Input = TracksTableMsg;
  type Output = TracksTableOutput;

  view! {
      gtk::Overlay {
          #[local_ref]
          #[wrap(Some)]
          set_child = tracks_table_view -> gtk::ColumnView {
              set_expand: true,
              set_show_column_separators: true,
              connect_activate => move |_cv, _row| {
                  sender.input(TracksTableMsg::HandleRowActivated);
              },
          },

          add_overlay = &adw::StatusPage {
            set_description: Some("No results"),
            set_icon_name: Some("edit-find-symbolic"),
            add_css_class: "compact",
            #[watch]
            set_visible: !model.rows_visible,
          },
      }
  }

  fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let mut table = TypedColumnView::<Track, gtk::MultiSelection>::new();

    // Append each column
    table.append_column::<TracksTableColumnArtist>();
    table.append_column::<TracksTableColumnAlbum>();
    table.append_column::<TracksTableColumnTrack>();
    table.append_column::<TracksTableColumnInstrumental>();
    table.append_column::<TracksTableColumnLyricsTag>();
    table.append_column::<TracksTableColumnLyricsSync>();
    table.append_column::<TracksTableColumnSidecar>();
    table.append_column::<TracksTableColumnChecked>();
    table.append_column::<TracksTableColumnModified>();

    // 0 = NeverChecked
    table.add_filter(|track| track.last_api_check_at.is_none());
    table.set_filter_status(0, false);

    // 1 = NoLyrics
    table.add_filter(|track| {
      track.lyrics.is_none()
        && track.lyrics_sidecar_lrc_file.is_none()
        && track.lyrics_sidecar_txt_file.is_none()
    });
    table.set_filter_status(1, false);

    // 2 = NoLyricsTag
    table.add_filter(|track| track.lyrics.is_none());
    table.set_filter_status(2, false);

    // 3 = NotInstrumental
    table
      .add_filter(|track| track.instrumental.is_none() || track.instrumental.is_some_and(|b| !b));
    table.set_filter_status(3, false);

    // 4 = NotSync
    table.add_filter(|track| !track.lyrics_synchronised && track.lyrics_sidecar_lrc_file.is_none());
    table.set_filter_status(4, false);

    // 5 = Lrc
    table.add_filter(|track| track.lyrics_sidecar_lrc_file.is_some());
    table.set_filter_status(5, false);

    // 6 = Txt
    table.add_filter(|track| track.lyrics_sidecar_txt_file.is_some());
    table.set_filter_status(6, false);

    // 7 = EitherLrcOrTxt
    table.add_filter(|track| {
      track.lyrics_sidecar_lrc_file.is_some() || track.lyrics_sidecar_txt_file.is_some()
    });
    table.set_filter_status(7, false);

    // Handle row selection
    let sender_handle = sender.clone();
    table
      .selection_model
      .connect_selection_changed(move |selection_model, _pos, _n_items| {
        let set = selection_model.selection();
        sender_handle.input(TracksTableMsg::HandleRowSelection(set));
      });

    // Handle key presses
    let sender_handle = sender.clone();
    let controller = EventControllerKey::new();
    controller.connect_key_pressed(move |_con, key, _idx, modifier| {
      trace!("TracksTable key event: key {key} + {:?}", modifier);
      if key == gtk::gdk::Key::Escape {
        sender_handle.input(TracksTableMsg::ClearSelection);
      }
      gtk::glib::Propagation::Proceed
    });
    table.view.add_controller(controller);

    let model = TracksTableModel {
      preset_filters_len: table.filters_len(),
      total_rows: 0,
      rows_visible: false,
      table,
    };

    // Ref of the view to use in `view!` macro
    let tracks_table_view = &model.table.view;

    let widgets = view_output!();
    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    match message {
      TracksTableMsg::HandleRowActivated => {
        sender
          .output(TracksTableOutput::RowActivated)
          .expect("receiver of TracksTableOutput dropped");
      }

      TracksTableMsg::HandleRowSelection(set) => {
        let mut selected_track_ids = HashSet::new();

        if let Some((mut iter, mut idx)) = BitsetIter::init_first(&set) {
          debug!("First row selected: {idx}");
          loop {
            let track_id = self
              .table
              .get_visible(idx)
              .map(|item| item.borrow().id)
              .expect("failed to get track ID");

            selected_track_ids.insert(track_id);

            if let Some(next_idx) = iter.next() {
              idx = next_idx;
            } else {
              debug!("Last row selected: {idx}");
              break;
            };
          }
        }

        sender
          .output(TracksTableOutput::TrackIdsSelected(selected_track_ids))
          .expect("receiver of TracksTableOutput dropped");
      }

      TracksTableMsg::ClearSelection => {
        debug!("Clearing selection");
        self.table.selection_model.unselect_all();
      }

      TracksTableMsg::ClearAndAppend(tracks) => {
        self.table.clear();
        self.table.extend_from_iter(tracks);
        self.reset_rows_state();
      }

      TracksTableMsg::Update(track) => {
        if let Some(idx) = self.table.find(|row| row.id == track.id) {
          self.table.remove(idx);
          self.table.append(track);
        }
      }

      TracksTableMsg::Filter(query) => {
        // Clear all dynamically-added filters
        while self.table.filters_len() > self.preset_filters_len {
          self.table.pop_filter();
        }

        if let Some(query) = query {
          for token in query.to_lowercase().split_whitespace().map(String::from) {
            self.table.add_filter(move |track| {
              track.artist_name.to_lowercase().contains(&token)
                || track.album_name.to_lowercase().contains(&token)
                || track.track_name.to_lowercase().contains(&token)
            });
          }
        }

        // Are any rows visible after filtering?
        self.set_rows_visible();
      }

      TracksTableMsg::SetFilter((filter, active)) => {
        // First disable `Lrc` and `Txt` filters if we want to show both
        if filter == TracksTableFilter::EitherLrcOrTxt && active {
          self.table.set_filter_status(5, false);
          self.table.set_filter_status(6, false);
        }

        debug!("Applying filter: {filter:?}");

        self.table.set_filter_status(filter as usize, active);

        // Are any rows visible after filtering?
        self.set_rows_visible();
      }

      TracksTableMsg::ClearFilters => {
        for idx in 0..self.table.filters_len() {
          self.table.set_filter_status(idx, false);
        }

        // Are any rows visible after filtering?
        self.set_rows_visible();
      }
    }
  }
}

impl TracksTableModel {
  fn reset_rows_state(&mut self) {
    self.total_rows = self.table.len();
    self.rows_visible = self.table.get_visible(0).is_some();
  }

  fn set_rows_visible(&mut self) {
    self.rows_visible = self.table.get_visible(0).is_some();
  }
}

// Column Models ///////////////////////////////////////////////////////////////

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

struct TracksTableColumnInstrumental;
impl RelmColumn for TracksTableColumnInstrumental {
  type Root = gtk::Image;
  type Widgets = ();
  type Item = Track;

  const COLUMN_NAME: &'static str = "Inst";

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let img = gtk::Image::new();
    (img, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    if item.instrumental.is_some_and(|b| b) {
      root.set_icon_name(Some("checkmark-symbolic"));
      root.set_tooltip("Marked as Instrumental");
    }
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.lyrics.is_some().cmp(&b.lyrics.is_some())))
  }
}

struct TracksTableColumnLyricsTag;
impl RelmColumn for TracksTableColumnLyricsTag {
  type Root = gtk::Image;
  type Widgets = ();
  type Item = Track;

  const COLUMN_NAME: &'static str = "Tag";

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let img = gtk::Image::new();
    (img, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    if item.lyrics.is_some() {
      root.set_icon_name(Some("checkmark-symbolic"));
      root.set_tooltip("Lyrics Tag Found");
    }
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.lyrics.is_some().cmp(&b.lyrics.is_some())))
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
      (Some(_), _) => {
        root.set_label("LRC");
        root.set_tooltip("Lyrics Sidecar File Format");
      }
      (None, Some(_)) => {
        root.set_label("TXT");
        root.set_tooltip("Lyrics Sidecar File Format");
      }
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

struct TracksTableColumnLyricsSync;
impl RelmColumn for TracksTableColumnLyricsSync {
  type Root = gtk::Image;
  type Widgets = ();
  type Item = Track;

  const COLUMN_NAME: &'static str = "Sync";

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let img = gtk::Image::new();
    (img, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    if item.lyrics_synchronised || item.lyrics_sidecar_lrc_file.is_some() {
      root.set_icon_name(Some("checkmark-symbolic"));
      root.set_tooltip("Lyrics Are Synchronised");
    }
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| {
      (a.lyrics_synchronised || a.lyrics_sidecar_lrc_file.is_some())
        .cmp(&(b.lyrics_synchronised || b.lyrics_sidecar_lrc_file.is_some()))
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
    let (label, tooltip) = if let Some(ndt) = item.last_api_check_at {
      let iso = &util::ndt_utc_to_ui_string(ndt);
      let label = if SETTINGS
        .read()
        .expect("settings lock is poisoned")
        .prefer_iso_timestamps
      {
        iso
      } else {
        &util::ndt_utc_to_humanised_string(ndt)
      };
      (label.to_string(), iso.to_string())
    } else {
      ("Never".into(), "Never Checked for Lyrics".into())
    };

    root.set_label(&label);
    root.set_tooltip(&tooltip);
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
    let iso = &util::ndt_utc_to_ui_string(item.file_modified_at);
    let label = if SETTINGS
      .read()
      .expect("settings lock is poisoned")
      .prefer_iso_timestamps
    {
      iso
    } else {
      &util::ndt_utc_to_humanised_string(item.file_modified_at)
    };
    root.set_label(label);
    root.set_tooltip(iso);
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.file_modified_at.cmp(&b.file_modified_at)))
  }
}
