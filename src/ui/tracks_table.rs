use std::collections::HashSet;

use relm4::gtk::gio::prelude::ListModelExt;
use relm4::gtk::glib::object::Cast;
use relm4::gtk::prelude::{BoxExt, SelectionModelExt, WidgetExt};
use relm4::gtk::{Bitset, BitsetIter, EventControllerKey, SortType};
use relm4::prelude::*;
use relm4::typed_view::column::{RelmColumn, TypedColumnView};
use tracing::{debug, error, trace};

use crate::SETTINGS;
use crate::settings::Settings;
use crate::track::Track;
use crate::util::{self};

pub struct TracksTableModel {
  table: TypedColumnView<Track, gtk::MultiSelection>,
  preset_filters_len: usize,
  total_rows: u32,
  is_row_visible: bool,
  iso_datetime_format: bool,
}

static COLUMN_TITLE_CHECKED: &str = "Checked";
static COLUMN_TITLE_MODIFIED: &str = "Modified";

#[derive(Debug)]
pub enum TracksTableMsg {
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
pub enum TracksTableOutput {
  TrackIdsSelected(HashSet<i32>),
  TrackIdsVisible(HashSet<i32>),
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
      #[wrap(Some)]
      set_child = &gtk::ScrolledWindow {
        set_expand: true,
        set_propagate_natural_height: true,
        set_valign: gtk::Align::Fill,
        set_policy: (gtk::PolicyType::Never, gtk::PolicyType::Automatic),

        #[local_ref]
        tracks_table_view -> gtk::ColumnView {
          set_expand: true,
          set_show_column_separators: true,
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
    let iso_datetime_format = SETTINGS
      .read()
      .inspect_err(|_| error!("Settings lock is poisoned while initialising TracksTable"))
      .map_or_else(
        |_| Settings::default().prefer_iso_timestamps,
        |g| g.prefer_iso_timestamps,
      );

    let table = create_table(&sender, iso_datetime_format);

    let model = TracksTableModel {
      preset_filters_len: table.filters_len(),
      total_rows: 0,
      is_row_visible: false,
      table,
      iso_datetime_format,
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
        let current_iso_datetime_format = SETTINGS
          .read()
          .inspect_err(|_| {
            error!("Settings lock is poisoned while calling ClearAndAppend on TracksTable");
          })
          .map_or(self.iso_datetime_format, |g| g.prefer_iso_timestamps);

        if current_iso_datetime_format != self.iso_datetime_format {
          debug!("Datetime format has changed; replacing columns");

          self.iso_datetime_format = !self.iso_datetime_format;

          // Remove existing columns
          let mut idx = 0;
          while let Some(col) = self.table.view.columns().item(idx) {
            if let Ok(col) = col.downcast::<gtk::ColumnViewColumn>() {
              if col.title().is_some_and(|t| {
                t.as_str() == COLUMN_TITLE_CHECKED || t.as_str() == COLUMN_TITLE_MODIFIED
              }) {
                self.table.view.remove_column(&col);
                trace!("Removed datetime column: {}", col.id().unwrap_or_default());
              } else {
                // Only increment if no column removed or we skip columns
                idx += 1;
              }
            }
          }

          // Append new columns
          if self.iso_datetime_format {
            self
              .table
              .append_column::<TracksTableColumnCheckedIsoFormat>();
            self
              .table
              .append_column::<TracksTableColumnModifiedIsoFormat>();
          } else {
            self
              .table
              .append_column::<TracksTableColumnCheckedSimpleFormat>();
            self
              .table
              .append_column::<TracksTableColumnModifiedSimpleFormat>();
          }
        }

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
          for token in query.to_lowercase().split_whitespace().map(String::from) {
            self.table.add_filter(move |track| {
              track.artist_name.to_lowercase().contains(&token)
                || track.album_name.to_lowercase().contains(&token)
                || track.track_name.to_lowercase().contains(&token)
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
) -> TypedColumnView<Track, gtk::MultiSelection> {
  let mut table = TypedColumnView::<Track, gtk::MultiSelection>::new();

  // Append columns
  table.append_column::<TracksTableColumnArtist>();
  table.append_column::<TracksTableColumnAlbum>();
  table.append_column::<TracksTableColumnTrack>();
  table.append_column::<TracksTableColumnLyricsTag>();
  table.append_column::<TracksTableColumnSidecar>();

  if iso_datetime_format {
    table.append_column::<TracksTableColumnCheckedIsoFormat>();
    table.append_column::<TracksTableColumnModifiedIsoFormat>();
  } else {
    table.append_column::<TracksTableColumnCheckedSimpleFormat>();
    table.append_column::<TracksTableColumnModifiedSimpleFormat>();
  }

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
  const ENABLE_EXPAND: bool = true;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_use_markup(false);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    root.set_label(&item.artist_name);
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
  const ENABLE_EXPAND: bool = true;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_use_markup(false);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    root.set_label(&item.album_name);
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
    bx.set_valign(gtk::Align::Center);

    let track_label = gtk::Label::new(None);
    track_label.set_align(gtk::Align::Start);
    track_label.set_xalign(0.0);
    track_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    track_label.set_use_markup(false);

    let inst_tag = gtk::Label::new(None);
    inst_tag.set_visible(false);

    bx.append(&track_label);
    bx.append(&inst_tag);

    (bx, (track_label, inst_tag))
  }

  fn bind(
    item: &mut Self::Item,
    (track_label, inst_tag): &mut Self::Widgets,
    _root: &mut Self::Root,
  ) {
    track_label.set_label(&item.track_name);
    track_label.set_tooltip(&item.path);

    if item.instrumental.is_some_and(|b| b) {
      inst_tag.set_label("INST");
      inst_tag.set_tooltip("Instrumental Track");
      inst_tag.add_css_class("caption");
      inst_tag.inline_css("padding: 0 0.5em; background: @card_bg_color; border-radius: 6px");
      inst_tag.set_visible(true);
    }
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| a.path.cmp(&b.path)))
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
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let icon = gtk::Image::new();

    bx.append(&icon);
    bx.append(&label);

    (bx, (icon, label))
  }

  fn bind(item: &mut Self::Item, (icon, label): &mut Self::Widgets, root: &mut Self::Root) {
    if item.lyrics.is_some() && item.lyrics_synchronised {
      label.set_label("Sync");
      label.inline_css("font-weight: bold");
      root.set_tooltip("Sync Lyrics Tag");
      icon.set_icon_name(Some("audio-x-generic-symbolic"));
    } else if item.lyrics.is_some() && !item.lyrics_synchronised {
      label.set_label("Plain");
      root.set_tooltip("Plain Lyrics Tag");
      icon.set_icon_name(Some("audio-x-generic-symbolic"));
      root.set_opacity(0.67);
    }
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

  const COLUMN_NAME: &'static str = "Sidecar";

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 6);

    let label = gtk::Label::new(None);
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let icon = gtk::Image::new();

    bx.append(&icon);
    bx.append(&label);

    (bx, (icon, label))
  }

  fn bind(item: &mut Self::Item, (icon, label): &mut Self::Widgets, root: &mut Self::Root) {
    if item.lyrics_sidecar_lrc_file.is_some() {
      label.set_label("Sync");
      label.inline_css("font-weight: bold");
      root.set_tooltip("Sync Sidecar File");
      icon.set_icon_name(Some("text-x-generic-symbolic"));
    } else if item.lyrics_sidecar_txt_file.is_some() {
      label.set_label("Plain");
      root.set_tooltip("Plain Sidecar File");
      icon.set_icon_name(Some("text-x-generic-symbolic"));
      root.set_opacity(0.67);
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

struct TracksTableColumnCheckedSimpleFormat;
impl RelmColumn for TracksTableColumnCheckedSimpleFormat {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = COLUMN_TITLE_CHECKED;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_width_chars(20);
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
    Some(Box::new(|a, b| {
      a.last_api_check_at.cmp(&b.last_api_check_at)
    }))
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
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_width_chars(20);
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

struct TracksTableColumnCheckedIsoFormat;
impl RelmColumn for TracksTableColumnCheckedIsoFormat {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = COLUMN_TITLE_CHECKED;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_width_chars(20);
    (label, ())
  }

  fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, root: &mut Self::Root) {
    let (label, tooltip) = if let Some(ndt) = item.last_api_check_at {
      let iso = util::ndt_utc_to_ui_string(ndt);
      (iso, None)
    } else {
      (
        "Never".to_string(),
        Some("Never Checked for Lyrics".to_string()),
      )
    };

    root.set_label(&label);

    if let Some(tooltip) = tooltip {
      root.set_tooltip(&tooltip);
    }
  }

  fn sort_fn() -> relm4::typed_view::OrdFn<Self::Item> {
    Some(Box::new(|a, b| {
      a.last_api_check_at.cmp(&b.last_api_check_at)
    }))
  }
}

struct TracksTableColumnModifiedIsoFormat;
impl RelmColumn for TracksTableColumnModifiedIsoFormat {
  type Item = Track;
  type Root = gtk::Label;
  type Widgets = ();

  const COLUMN_NAME: &'static str = COLUMN_TITLE_MODIFIED;

  fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
    let label = gtk::Label::new(None);
    label.set_align(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_width_chars(20);
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
