use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use bstr::ByteSlice;
use relm4::gtk::prelude::{BoxExt, SelectionModelExt, WidgetExt};
use relm4::gtk::{Bitset, BitsetIter, EventControllerKey, SortType};
use relm4::prelude::*;
use relm4::typed_view::column::{RelmColumn, TypedColumnView};
use tracing::{debug, error, trace};

use crate::SETTINGS;
use crate::lyrics::LyricsType;
use crate::settings::Settings;
use crate::track::Track;
use crate::util::{self};

pub(crate) struct TracksTableModel {
  table: TypedColumnView<Track, gtk::MultiSelection>,
  preset_filters_len: usize,
  total_rows: u32,
  is_row_visible: bool,
  prefer_accurate_timestamps: bool,
}

static COLUMN_TITLE_SIDECAR: &str = "Sidecar";
static COLUMN_TITLE_CHECKED: &str = "Checked";
static COLUMN_TITLE_MODIFIED: &str = "Modified";

static PREFER_SYNC_LYRICS: AtomicBool = AtomicBool::new(true);

#[derive(Debug)]
pub(crate) enum TracksTableMsg {
  ClearAndAppend(Vec<Track>),
  Update(Box<Track>),
  Filter(Option<String>),
  SetFilter((TracksTableFilter, bool)),
  ClearFilters,
  RefreshTrackIdsVisible,
  HandleRowActivated,
  HandleRowSelection(Bitset),
  ClearSelection,
}

#[derive(Debug)]
pub(crate) enum TracksTableOutput {
  TrackIdsSelected(HashSet<i32>),
  TrackIdsVisible(HashSet<i32>),
  RowActivated,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum TracksTableFilter {
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
      #[wrap(Some)]
      set_child = &gtk::ScrolledWindow {
        set_expand: true,
        set_propagate_natural_height: true,
        set_valign: gtk::Align::Fill,
        set_policy: (gtk::PolicyType::Never, gtk::PolicyType::Automatic),

        #[local_ref]
        tracks_table_view -> gtk::ColumnView {
          set_expand: true,
          set_reorderable: false,
          connect_activate => move |_cv, _row| {
            sender.input(TracksTableMsg::HandleRowActivated);
          },
        },
      },

      add_overlay = &adw::StatusPage {
        set_description: Some("No results"),
        set_icon_name: Some("edit-find-symbolic"),
        add_css_class: "compact",
        #[watch]
        set_visible: !model.is_row_visible,
      },
    }
  }

  fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let (prefer_accurate_timestamps, prefer_lyrics_type, col_separators, row_separators) = SETTINGS
      .read()
      .inspect_err(|_| error!("Settings lock is poisoned while initialising TracksTable"))
      .map_or_else(
        |_| {
          let default = Settings::default();
          (
            default.prefer_accurate_timestamps,
            default.prefer_lyrics_type,
            default.tracks_table_col_separators,
            default.tracks_table_row_separators,
          )
        },
        |g| {
          (
            g.prefer_accurate_timestamps,
            g.prefer_lyrics_type,
            g.tracks_table_col_separators,
            g.tracks_table_row_separators,
          )
        },
      );

    PREFER_SYNC_LYRICS.store(prefer_lyrics_type == LyricsType::Sync, Ordering::SeqCst);

    let table = create_table(&sender, prefer_accurate_timestamps, col_separators, row_separators);

    let model = TracksTableModel {
      preset_filters_len: table.filters_len(),
      total_rows: 0,
      is_row_visible: false,
      table,
      prefer_accurate_timestamps,
    };

    // Ref of the `ColumnView` to use in `view` macro
    let tracks_table_view = &model.table.view;

    let widgets = view_output!();

    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    match message {
      TracksTableMsg::HandleRowActivated => {
        sender
          .output(TracksTableOutput::RowActivated)
          .expect("TracksTableOutput receiver dropped");
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
            }
          }
        }

        sender
          .output(TracksTableOutput::TrackIdsSelected(selected_track_ids))
          .expect("TracksTableOutput receiver dropped");
      }

      TracksTableMsg::RefreshTrackIdsVisible => {
        let mut visible_track_ids = HashSet::new();

        let mut idx = 0_u32;
        while let Some(item) = self.table.get_visible(idx) {
          visible_track_ids.insert(item.borrow().id);
          idx += 1;
        }

        sender
          .output(TracksTableOutput::TrackIdsVisible(visible_track_ids))
          .expect("TracksTableOutput receiver dropped");
      }

      TracksTableMsg::ClearSelection => {
        debug!("Clearing selection");
        self.table.selection_model.unselect_all();
      }

      TracksTableMsg::ClearAndAppend(tracks) => {
        self.table.clear();

        // Replace datetime columns if format has changed
        let (prefer_accurate_timestamps, prefer_lyrics_type, col_separators, row_separators) =
          SETTINGS
            .read()
            .inspect_err(|_| {
              error!("Settings lock is poisoned while calling `ClearAndAppend` on TracksTable");
            })
            .map_or_else(
              |_| {
                let default = Settings::default();
                (
                  default.prefer_accurate_timestamps,
                  default.prefer_lyrics_type,
                  default.tracks_table_col_separators,
                  default.tracks_table_row_separators,
                )
              },
              |g| {
                (
                  g.prefer_accurate_timestamps,
                  g.prefer_lyrics_type,
                  g.tracks_table_col_separators,
                  g.tracks_table_row_separators,
                )
              },
            );

        // Apply row/column separator settings
        self.table.view.set_show_column_separators(col_separators);
        self.table.view.set_show_row_separators(row_separators);

        // Re-add 'Sidecar' column to use the appropriate sort function defined in the `RelmColumn` impl
        if PREFER_SYNC_LYRICS.load(Ordering::Relaxed) != (prefer_lyrics_type == LyricsType::Sync) {
          debug!("Preferred lyrics type has changed; replacing sidecar column");

          PREFER_SYNC_LYRICS.store(prefer_lyrics_type == LyricsType::Sync, Ordering::SeqCst);

          // Remove existing column
          let columns = self.table.get_columns();
          if let Some(col_sidecar) = columns.get(COLUMN_TITLE_SIDECAR) {
            // Hide table during column changes
            self.table.view.set_visible(false);

            self.table.view.remove_column(col_sidecar);

            // Append new column
            self.table.append_column::<TracksTableColumnSidecar>();

            // Reorder column
            let columns = self.table.get_columns();
            if let Some(col_sidecar) = columns.get(COLUMN_TITLE_SIDECAR) {
              self.table.view.remove_column(col_sidecar);
              self.table.view.insert_column(4, col_sidecar);
            }
          }
        }

        // Replace datetime columns if preferred format has changed
        if prefer_accurate_timestamps != self.prefer_accurate_timestamps {
          debug!("Datetime format has changed; replacing columns");

          self.prefer_accurate_timestamps = prefer_accurate_timestamps;

          // Remove existing columns
          let columns = self.table.get_columns();
          if let (Some(col_checked), Some(col_modified)) =
            (columns.get(COLUMN_TITLE_CHECKED), columns.get(COLUMN_TITLE_MODIFIED))
          {
            // Hide table during column changes
            self.table.view.set_visible(false);

            self.table.view.remove_column(col_checked);
            self.table.view.remove_column(col_modified);

            // Append new columns
            if self.prefer_accurate_timestamps {
              self
                .table
                .append_column::<TracksTableColumnCheckedAccurateFormat>();
              self
                .table
                .append_column::<TracksTableColumnModifiedAccurateFormat>();
            } else {
              self
                .table
                .append_column::<TracksTableColumnCheckedSimpleFormat>();
              self
                .table
                .append_column::<TracksTableColumnModifiedSimpleFormat>();
            }
          }
        }

        // Restore table in case it was hidden for column changes
        self.table.view.set_visible(true);

        self.table.extend_from_iter(tracks);

        self.reset_rows_state();

        sender.input(TracksTableMsg::RefreshTrackIdsVisible);
      }

      TracksTableMsg::Update(track) => {
        if let Some(idx) = self.table.find(|row| row.id == track.id) {
          self.table.remove(idx);
          self.table.insert(idx, *track);
        }
      }

      TracksTableMsg::Filter(query) => {
        // Clear all dynamically-added filters
        while self.table.filters_len() > self.preset_filters_len {
          self.table.pop_filter();
        }

        if let Some(query) = query {
          let segments = query
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();

          for segment in segments {
            self.table.add_filter(move |track| {
              track.path.as_bytes().to_lowercase().contains_str(&segment)
                || track
                  .artist_name
                  .as_bytes()
                  .to_lowercase()
                  .contains_str(&segment)
                || track
                  .album_name
                  .as_bytes()
                  .to_lowercase()
                  .contains_str(&segment)
                || track
                  .track_name
                  .as_bytes()
                  .to_lowercase()
                  .contains_str(&segment)
            });
          }
        }

        // Are any rows visible after filtering?
        self.update_is_row_visible();

        sender.input(TracksTableMsg::RefreshTrackIdsVisible);
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
        self.update_is_row_visible();

        sender.input(TracksTableMsg::RefreshTrackIdsVisible);
      }

      TracksTableMsg::ClearFilters => {
        for idx in 0..self.table.filters_len() {
          self.table.set_filter_status(idx, false);
        }

        // Are any rows visible after filtering?
        self.update_is_row_visible();

        sender.input(TracksTableMsg::RefreshTrackIdsVisible);
      }
    }
  }
}

impl TracksTableModel {
  fn reset_rows_state(&mut self) {
    self.total_rows = self.table.len();
    self.is_row_visible = self.table.get_visible(0).is_some();
  }

  fn update_is_row_visible(&mut self) {
    self.is_row_visible = self.table.get_visible(0).is_some();
  }
}

fn create_table(
  sender: &ComponentSender<TracksTableModel>,
  iso_datetime_format: bool,
  col_separators: bool,
  row_separators: bool,
) -> TypedColumnView<Track, gtk::MultiSelection> {
  let mut table = TypedColumnView::<Track, gtk::MultiSelection>::new();

  // Append columns
  table.append_column::<TracksTableColumnArtist>();
  table.append_column::<TracksTableColumnAlbum>();
  table.append_column::<TracksTableColumnTrack>();
  table.append_column::<TracksTableColumnLyricsTag>();
  table.append_column::<TracksTableColumnSidecar>();

  if iso_datetime_format {
    table.append_column::<TracksTableColumnCheckedAccurateFormat>();
    table.append_column::<TracksTableColumnModifiedAccurateFormat>();
  } else {
    table.append_column::<TracksTableColumnCheckedSimpleFormat>();
    table.append_column::<TracksTableColumnModifiedSimpleFormat>();
  }

  // Set column widths
  for (&name, col) in table.get_columns() {
    match name {
      "Artist" | "Album" => col.set_fixed_width(180),
      "Track" => col.set_expand(true),
      // Other columns will use their natural width
      _ => {}
    }
  }

  // Set preferred separators
  table.view.set_show_column_separators(col_separators);
  table.view.set_show_row_separators(row_separators);

  // Add preset filters used with the toggle 'chips' below the search input

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
  table.add_filter(|track| track.instrumental.is_none_or(|b| !b));
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

  // Apply default sorting
  let artist_col = table
    .get_columns()
    .get("Artist")
    .expect("TracksTable should have Artist column");
  table
    .view
    .sort_by_column(Some(artist_col), SortType::Ascending);

  table
}

// Column Models ///////////////////////////////////////////////////////////////

struct TracksTableColumnArtist;
impl RelmColumn for TracksTableColumnArtist {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = "Artist";
  const ENABLE_RESIZE: bool = true;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    label.set_use_markup(false);
    label.set_width_chars(0);
    label.set_max_width_chars(0);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, label: &mut Self::Root) {
    label.set_label(&item.artist_name);
    label.set_tooltip(&item.path);
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| {
      a.artist_name
        .cmp(&b.artist_name)
        .then_with(|| a.album_name.cmp(&b.album_name))
        .then_with(|| a.path.cmp(&b.path))
    }))
  }
}

struct TracksTableColumnAlbum;
impl RelmColumn for TracksTableColumnAlbum {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = "Album";
  const ENABLE_RESIZE: bool = true;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    label.set_use_markup(false);
    label.set_width_chars(0);
    label.set_max_width_chars(0);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, label: &mut Self::Root) {
    label.set_label(&item.album_name);
    label.set_tooltip(&item.path);
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| {
      a.album_name
        .cmp(&b.album_name)
        .then_with(|| a.path.cmp(&b.path))
    }))
  }
}

struct TracksTableColumnTrack;
impl RelmColumn for TracksTableColumnTrack {
  type Root = gtk::Box;
  type Widgets = (gtk::Label, gtk::Label);
  type Item = Track;

  const COLUMN_NAME: &'static str = "Track";

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bx.set_hexpand(true);
    bx.set_valign(gtk::Align::Center);

    let label = gtk::Label::new(None);
    label.set_hexpand(false);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    label.set_use_markup(false);

    let inst_tag = gtk::Label::new(None);
    inst_tag.add_css_class("caption");
    inst_tag.inline_css("padding: 0 0.5em; background: @sidebar_bg_color; border-radius: 6px");
    inst_tag.set_label("INST");
    inst_tag.set_tooltip("Instrumental Track");
    inst_tag.set_visible(false);

    bx.append(&label);
    bx.append(&inst_tag);

    (bx, (label, inst_tag))
  }

  fn bind(item: &mut Self::Item, (label, inst_tag): &mut Self::Widgets, _root: &mut Self::Root) {
    label.set_label(&item.track_name);
    label.set_tooltip(&item.path);

    if item.instrumental.unwrap_or(false) {
      inst_tag.set_visible(true);
    }
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.track_name.cmp(&b.track_name).then(a.path.cmp(&b.path))))
  }
}

struct TracksTableColumnLyricsTag;
impl RelmColumn for TracksTableColumnLyricsTag {
  type Root = gtk::Box;
  type Widgets = (gtk::Image, gtk::Label);
  type Item = Track;

  const COLUMN_NAME: &'static str = "Tag";

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 6);

    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let icon = gtk::Image::new();

    bx.append(&icon);
    bx.append(&label);

    bx.set_css_classes(&["table-cell", "lyrics"]);

    (bx, (icon, label))
  }

  fn bind(item: &mut Self::Item, (icon, label): &mut Self::Widgets, root: &mut Self::Root) {
    if item.lyrics.is_some() && item.lyrics_synchronised {
      label.set_label("Sync");
      root.set_tooltip("Sync Lyrics Tag");
      icon.set_icon_name(Some("audio-x-generic-symbolic"));
      if PREFER_SYNC_LYRICS.load(Ordering::Relaxed) {
        root.add_css_class("preferred");
      }
    } else if item.lyrics.is_some() && !item.lyrics_synchronised {
      label.set_label("Plain");
      root.set_tooltip("Plain Lyrics Tag");
      icon.set_icon_name(Some("audio-x-generic-symbolic"));
      if !PREFER_SYNC_LYRICS.load(Ordering::Relaxed) {
        root.add_css_class("preferred");
      }
    }
  }

  fn unbind(_item: &mut Self::Item, (icon, label): &mut Self::Widgets, root: &mut Self::Root) {
    icon.set_icon_name(None);
    label.set_label("");
    root.set_tooltip("");
    root.remove_css_class("preferred");
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| {
      a.lyrics
        .is_some()
        .cmp(&b.lyrics.is_some())
        .then_with(|| a.lyrics_synchronised.cmp(&b.lyrics_synchronised))
    }))
  }
}

struct TracksTableColumnSidecar;
impl RelmColumn for TracksTableColumnSidecar {
  type Root = gtk::Box;
  type Widgets = (gtk::Image, gtk::Label);
  type Item = Track;

  const COLUMN_NAME: &'static str = COLUMN_TITLE_SIDECAR;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 6);

    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let icon = gtk::Image::new();

    bx.append(&icon);
    bx.append(&label);

    bx.set_css_classes(&["table-cell", "lyrics"]);

    (bx, (icon, label))
  }

  fn bind(item: &mut Self::Item, (icon, label): &mut Self::Widgets, root: &mut Self::Root) {
    if item.lyrics_sidecar_lrc_file.is_some() && item.lyrics_sidecar_txt_file.is_some() {
      if PREFER_SYNC_LYRICS.load(Ordering::Relaxed) {
        label.set_label("Sync+");
        root.set_tooltip("Multiple Sidecar Files");
        root.add_css_class("preferred");
        icon.set_icon_name(Some("text-x-generic-symbolic"));
      } else {
        label.set_label("Plain+");
        root.set_tooltip("Multiple Sidecar Files");
        root.add_css_class("preferred");
        icon.set_icon_name(Some("text-x-generic-symbolic"));
      }
    } else if item.lyrics_sidecar_lrc_file.is_some() {
      label.set_label("Sync");
      root.set_tooltip("Sync Sidecar File");
      icon.set_icon_name(Some("text-x-generic-symbolic"));
      if PREFER_SYNC_LYRICS.load(Ordering::Relaxed) {
        root.add_css_class("preferred");
      }
    } else if item.lyrics_sidecar_txt_file.is_some() {
      label.set_label("Plain");
      root.set_tooltip("Plain Sidecar File");
      icon.set_icon_name(Some("text-x-generic-symbolic"));
      if !PREFER_SYNC_LYRICS.load(Ordering::Relaxed) {
        root.add_css_class("preferred");
      }
    }
  }

  fn unbind(_item: &mut Self::Item, (icon, label): &mut Self::Widgets, root: &mut Self::Root) {
    icon.set_icon_name(None);
    label.set_label("");
    root.set_tooltip("");
    root.remove_css_class("preferred");
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    if PREFER_SYNC_LYRICS.load(Ordering::Relaxed) {
      Some(Box::new(|a, b| {
        (a.lyrics_sidecar_lrc_file.is_some() && a.lyrics_sidecar_txt_file.is_some())
          .cmp(&(b.lyrics_sidecar_lrc_file.is_some() && b.lyrics_sidecar_txt_file.is_some()))
          .then_with(|| {
            a.lyrics_sidecar_lrc_file
              .is_some()
              .cmp(&b.lyrics_sidecar_lrc_file.is_some())
          })
          .then_with(|| {
            a.lyrics_sidecar_txt_file
              .is_some()
              .cmp(&b.lyrics_sidecar_txt_file.is_some())
          })
      }))
    } else {
      Some(Box::new(|a, b| {
        (a.lyrics_sidecar_lrc_file.is_some() && a.lyrics_sidecar_txt_file.is_some())
          .cmp(&(b.lyrics_sidecar_lrc_file.is_some() && b.lyrics_sidecar_txt_file.is_some()))
          .then_with(|| {
            a.lyrics_sidecar_txt_file
              .is_some()
              .cmp(&b.lyrics_sidecar_txt_file.is_some())
          })
          .then_with(|| {
            a.lyrics_sidecar_lrc_file
              .is_some()
              .cmp(&b.lyrics_sidecar_lrc_file.is_some())
          })
      }))
    }
  }
}

struct TracksTableColumnCheckedSimpleFormat;
impl RelmColumn for TracksTableColumnCheckedSimpleFormat {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = COLUMN_TITLE_CHECKED;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_hexpand(false);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_width_chars(20);
    label.set_single_line_mode(true);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    let (label, tooltip) = if let Some(ndt) = item.last_api_check_at {
      let iso = util::ndt_utc_to_ui_string(ndt);
      let label = util::ndt_utc_to_humanised_string(ndt);
      (label, iso)
    } else {
      ("Never".into(), "Never Checked for Lyrics".into())
    };

    root.set_label(&label);
    root.set_tooltip(&tooltip);
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.last_api_check_at.cmp(&b.last_api_check_at)))
  }
}

struct TracksTableColumnModifiedSimpleFormat;
impl RelmColumn for TracksTableColumnModifiedSimpleFormat {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = COLUMN_TITLE_MODIFIED;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_hexpand(false);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_width_chars(20);
    label.set_single_line_mode(true);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    let iso = util::ndt_utc_to_ui_string(item.file_modified_at);
    let label = util::ndt_utc_to_humanised_string(item.file_modified_at);
    root.set_label(&label);
    root.set_tooltip(&iso);
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.file_modified_at.cmp(&b.file_modified_at)))
  }
}

struct TracksTableColumnCheckedAccurateFormat;
impl RelmColumn for TracksTableColumnCheckedAccurateFormat {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = COLUMN_TITLE_CHECKED;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_hexpand(false);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    let (label, tooltip) = if let Some(ndt) = item.last_api_check_at {
      let iso = util::ndt_utc_to_ui_string(ndt);
      (iso, None)
    } else {
      ("Never".to_string(), Some("Never Checked for Lyrics".to_string()))
    };

    root.set_label(&label);

    if let Some(tooltip) = tooltip {
      root.set_tooltip(&tooltip);
    }
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.last_api_check_at.cmp(&b.last_api_check_at)))
  }
}

struct TracksTableColumnModifiedAccurateFormat;
impl RelmColumn for TracksTableColumnModifiedAccurateFormat {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = COLUMN_TITLE_MODIFIED;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_hexpand(false);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    let iso = util::ndt_utc_to_ui_string(item.file_modified_at);
    root.set_label(&iso);
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.file_modified_at.cmp(&b.file_modified_at)))
  }
}
