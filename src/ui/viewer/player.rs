use std::{
  fmt::Debug,
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use relm4::gtk::{GestureClick, gio, glib};
use relm4::prelude::*;
use relm4::{adw::prelude::*, gtk::gdk};
use rodio::Source;
use tokio::sync::oneshot;
use tracing::{debug, error, trace, warn};

use crate::{track::Track, util};

const COVER_ART_SIZE: i32 = 90;

struct RodioPlayer(Arc<rodio::Player>);

impl Debug for RodioPlayer {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "rodio `Player`")
  }
}

#[derive(Debug)]
pub(super) struct PlayerModel {
  player: Option<RodioPlayer>,
  player_task_handle: Option<tokio::task::JoinHandle<()>>,
  player_task_cancel: Option<oneshot::Sender<()>>,
  state: PlayerState,
  position: f64,
  length: f64,
  timestamp_pos: String,
  cover: Option<gdk::Texture>,
}

#[derive(Debug)]
pub(super) enum PlayerMsg {
  TogglePlay,
  UpdatePosition(f64),
  /// Seek to a point in the track, where `0.0` is 0% and `1.0` is 100%.
  Seek(f64),
  PlaybackEnded,
  CloseRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlayerState {
  Paused,
  Playing,
}

#[relm4::component(pub, async)]
impl SimpleAsyncComponent for PlayerModel {
  type Input = PlayerMsg;
  type Output = ();
  type Init = Rc<Track>;

  view! {
    gtk::Box {
      inline_css: "background: @sidebar_bg_color;",
      set_vexpand: false,
      set_hexpand: false,

      // Left box - cover art
      gtk::Box {
        set_visible: model.cover.is_some(),
        set_expand: false,

        gtk::Image {
          inline_css: "background: lightgrey;",
          set_paintable: model.cover.as_ref(),
          set_overflow: gtk::Overflow::Hidden,
          set_pixel_size: COVER_ART_SIZE,
        },
      },

      // Right box - controls
      gtk::Box {
        set_valign: gtk::Align::Center,
        set_spacing: 12,
        set_margin_start: 24,
        set_margin_end: 12,
        set_margin_vertical: 12,

        gtk::Button {
          inline_css: "border-radius: 1000px;",
          set_icon_name: "media-skip-backward-symbolic",
          set_tooltip: "Skip to Start",

          connect_clicked[sender] => move |_btn| {
            sender.input(PlayerMsg::Seek(0.0));
          },
        },

        gtk::Button {
          inline_css: "border-radius: 1000px;",
          #[watch]
          set_icon_name: if let PlayerState::Paused = model.state
            { "media-playback-start-symbolic" }
            else { "media-playback-pause-symbolic"},
          #[watch]
          set_tooltip: if let PlayerState::Paused = model.state
            { "Play" }
            else { "Pause"},

          connect_clicked[sender] => move |_btn| {
            sender.input(PlayerMsg::TogglePlay);
          },
        },
      },

      gtk::Box {
        set_valign: gtk::Align::Center,
        set_hexpand: true,
        set_margin_end: 24,
        set_margin_start: 12,
        set_margin_vertical: 12,

        #[local_ref]
        progress_bar -> gtk::ProgressBar {
          set_hexpand: true,
          set_show_text: true,
          add_css_class: "seekbar",

          #[watch]
          set_text: Some(&model.timestamp_pos),

          #[watch]
          set_fraction: if model.length == 0.0 { 0.0 }
            else { model.position / model.length },
        },
      },
    },
  }

  async fn init(
    track: Self::Init,
    root: Self::Root,
    sender: AsyncComponentSender<Self>,
  ) -> AsyncComponentParts<Self> {
    let path = track.path();

    // Open audio file, create the decoder and open the default sound output device
    let model = if let Ok(file) = std::fs::File::open(path.clone()).inspect_err(|e| warn!("{e}"))
      && let Ok(mut source) = rodio::Decoder::try_from(file).inspect_err(|e| warn!("{e}"))
      && let Ok(mut output) =
        rodio::DeviceSinkBuilder::open_default_sink().inspect_err(|e| warn!("{e}"))
    {
      output.log_on_drop(false);

      let length = source
        .by_ref()
        .total_duration()
        .map_or(0.0, |dur| dur.as_secs_f64());

      let player = Arc::new(rodio::Player::connect_new(output.mixer()));
      player.append(source);
      player.pause();

      let player_handle = Arc::clone(&player);
      let sender_handle = sender.clone();

      let (cancel_tx, mut cancel_rx) = oneshot::channel();

      // The background player task
      let task = tokio::spawn(async move {
        // Keep device sink handle alive
        let _output = output;

        let mut tick = tokio::time::interval(Duration::from_millis(33));

        let mut previous_second = 0;
        let mut last_pos_update = Instant::now();

        loop {
          tokio::select! {
            _ = &mut cancel_rx => {
              trace!("Player task cancelling...");

              break;
            }

            _ = tick.tick() => {
              if player_handle.empty() {
                debug!("Playback ended");

                // Re-append the source so playback can be restarted
                if let Ok(file) = std::fs::File::open(path.clone()).inspect_err(|e| warn!("{e}"))
                  && let Ok(source) = rodio::Decoder::try_from(file).inspect_err(|e| warn!("{e}")) {
                    player_handle.append(source);
                    player_handle.pause();

                    sender_handle.input(PlayerMsg::PlaybackEnded);
                  } else {
                    // Abort task on failure to re-append source
                    error!("Unable to re-append file to playback queue");

                    sender_handle.input(PlayerMsg::PlaybackEnded);
                    break;
                  }
              } else if !player_handle.is_paused() {
                // Update progress for every new whole second elapsed
                let current_second = player_handle.get_pos().as_secs();
                if current_second != previous_second {
                  sender_handle.input(PlayerMsg::UpdatePosition(player_handle.get_pos().as_secs_f64()));

                  previous_second = current_second;
                  last_pos_update = Instant::now();
                }

                // Update progress more frequently between seconds for smoother feedback on seekbar
                if last_pos_update.elapsed().as_millis() >= 250 {
                  sender_handle.input(PlayerMsg::UpdatePosition(player_handle.get_pos().as_secs_f64()));

                  last_pos_update = Instant::now();
                }
              }
            }
          }
        }
      });

      // Extract cover art
      let cover = || {
        let file = std::fs::File::open(track.path()).ok()?;
        let bytes = Track::get_cover_art_bytes_for_file(file).ok()?;
        let texture = scale_cover_art_to_texture(&bytes, COVER_ART_SIZE)?;
        // let bytes = glib::Bytes::from(bytes.as_slice());
        // let texture = gdk::Texture::from_bytes(&bytes).ok()?;
        Some(texture)
      };

      PlayerModel {
        player: Some(RodioPlayer(player)),
        player_task_cancel: Some(cancel_tx),
        player_task_handle: Some(task),
        state: PlayerState::Paused,
        position: 0.0,
        length,
        timestamp_pos: util::secs_f64_to_hms(0.0),
        cover: cover(),
      }
    } else {
      warn!("Failed to open audio stream for {track} - player will be hidden");

      // Hide the player if we fail to decode the file or open the sound output device
      root.set_visible(false);

      PlayerModel {
        player: None,
        player_task_cancel: None,
        player_task_handle: None,
        state: PlayerState::Paused,
        position: 0.0,
        length: 0.0,
        timestamp_pos: String::new(),
        cover: None,
      }
    };

    let gesture_click = GestureClick::new();
    let sender_handle = sender.clone();
    gesture_click.connect_pressed(move |gesture, _btn, x, _y| {
      if let Some(width) = gesture.widget().map(|widget| f64::from(widget.width()))
        && width > 0.0
      {
        let pos = x / width;
        sender_handle.input(PlayerMsg::Seek(pos));
      }
    });

    let progress_bar = &gtk::ProgressBar::new();
    progress_bar.add_controller(gesture_click);

    let widgets = view_output!();

    AsyncComponentParts { model, widgets }
  }

  async fn update(&mut self, message: Self::Input, sender: AsyncComponentSender<Self>) {
    match message {
      PlayerMsg::TogglePlay => {
        if let Some(player) = self.player.as_ref() {
          if player.0.is_paused() {
            debug!("Starting playback");

            player.0.play();
            self.state = PlayerState::Playing;
          } else {
            debug!("Stopping playback");

            player.0.pause();
            self.state = PlayerState::Paused;
          }
        }
      }

      PlayerMsg::UpdatePosition(pos) => {
        self.update_position_and_timestamp(pos);
      }

      PlayerMsg::Seek(pos) => {
        let pos = pos.clamp(0.0, 1.0);
        let secs = pos * self.length;

        debug!("Seeking to position {secs:.2}s");

        self.player.as_ref().inspect(|p| {
          let _ = p
            .0
            .try_seek(std::time::Duration::from_secs_f64(secs))
            .inspect_err(|e| warn!("Failed to seek to position {secs:.2}s: {e}"));
        });

        self.update_position_and_timestamp(pos);
      }

      PlayerMsg::PlaybackEnded => {
        self.state = PlayerState::Paused;

        sender.input(PlayerMsg::Seek(0.0));
      }

      PlayerMsg::CloseRequested => {
        // Ensure playback stops if the view lyrics window is closed
        if let Some(player) = self.player.as_ref() {
          player.0.clear();
        }

        // Cancel the background player task
        if let Some(cancel_tx) = self.player_task_cancel.take()
          && let Some(task) = self.player_task_handle.take()
        {
          let _ = cancel_tx.send(());
          let _ = task.await;

          trace!("Player task cancelled");
        }
      }
    }
  }
}

impl PlayerModel {
  fn update_position_and_timestamp(&mut self, pos: f64) {
    self.position = pos;

    let timestamp = util::secs_f64_to_hms(pos);

    if self.timestamp_pos != timestamp {
      self.timestamp_pos = timestamp;
    }
  }
}

impl Drop for PlayerModel {
  fn drop(&mut self) {
    if let Some(cancel_tx) = self.player_task_cancel.take() {
      let _ = cancel_tx.send(());
    } else if let Some(task) = self.player_task_handle.take() {
      task.abort();
    }
  }
}

fn scale_cover_art_to_texture(bytes: &[u8], max_size: i32) -> Option<gdk::Texture> {
  let bytes = glib::Bytes::from(bytes);
  let stream = gio::MemoryInputStream::from_bytes(&bytes);

  let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
    &stream,
    max_size,
    max_size,
    true,
    gio::Cancellable::NONE,
  )
  .ok()?;

  #[allow(deprecated)]
  Some(gdk::Texture::for_pixbuf(&pixbuf))
}
