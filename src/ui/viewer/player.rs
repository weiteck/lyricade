use std::{fmt::Debug, rc::Rc, sync::Arc, time::Duration};

use relm4::adw::prelude::*;
use relm4::prelude::*;
use rodio::Source;
use tokio::sync::oneshot;
use tracing::{debug, trace, warn};

use crate::{track::Track, util};

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
  cover: Option<gtk::gdk::Texture>,
}

#[derive(Debug)]
pub(super) enum PlayerMsg {
  TogglePlay,
  UpdatePosition(f64),
  Seek(f64),
  HandlePaused,
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
          set_pixel_size: 90,
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

        gtk::ProgressBar {
          set_hexpand: true,
          set_show_text: true,
          set_tooltip: &model.timestamp_pos,

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
    // Open audio file, create the decoder and open the default sound output device
    let model = if let Ok(file) = std::fs::File::open(track.path()).inspect_err(|e| warn!("{e}"))
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

      let task = tokio::spawn(async move {
        // Keep device sink handle alive
        let _output = output;

        let mut tick = tokio::time::interval(Duration::from_millis(100));

        loop {
          tokio::select! {
            _ = &mut cancel_rx => {
              trace!("Player task cancelling...");

              break;
            }

            _ = tick.tick() => {
              if player_handle.is_paused() {
                sender_handle.input(PlayerMsg::HandlePaused);
              } else {
                sender_handle.input(PlayerMsg::UpdatePosition(player_handle.get_pos().as_secs_f64()));
              }
            }
          }
        }
      });

      // Extract cover art
      let cover = || {
        let file = std::fs::File::open(track.path()).ok()?;
        let bytes = Track::get_cover_art_bytes_for_file(file).ok()?;
        let bytes = gtk::glib::Bytes::from(bytes.as_slice());
        let texture = gtk::gdk::Texture::from_bytes(&bytes).ok()?;
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

    let widgets = view_output!();

    AsyncComponentParts { model, widgets }
  }

  async fn update(&mut self, message: Self::Input, _sender: AsyncComponentSender<Self>) {
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
        debug!("Seeking to position {pos:.2}s");

        self.player.as_ref().inspect(|p| {
          let _ = p
            .0
            .try_seek(std::time::Duration::from_secs_f64(pos))
            .inspect_err(|e| warn!("Failed to seek to position {pos:.2}s: {e}"));
        });

        self.update_position_and_timestamp(pos);
      }

      PlayerMsg::HandlePaused => {
        if let PlayerState::Playing = self.state {
          debug!("Playback ended");

          self.state = PlayerState::Paused;
        }
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
