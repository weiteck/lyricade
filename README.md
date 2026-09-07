<p align="center">
  <img src="https://raw.githubusercontent.com/weiteck/lyricade/refs/heads/main/data/icons/io.github.weiteck.Lyricade.svg" width="128" alt="Lyricade icon">
</p>

# Lyricade

A Linux desktop application for fetching and managing lyrics in your local music library. It will scan your music and download missing lyrics, in either synchronised LRC or plain text format, at your preference.

**Lyricade** is a modern GTK4/libadwaita application that aims to follow GNOME Human Interface Guidelines.

<p>
  <a href="https://flathub.org/apps/io.github.weiteck.Lyricade"><img src="https://flathub.org/assets/badges/flathub-badge-en.png" width="200"/></a>
</p>

## Features

- Scan your music files and find missing lyrics
- Download either synchronised LRC or plain TXT lyrics
- Embed lyrics to the metadata tag and/or save as a sidecar file
- Lyrics viewer with playback and lyric highlighting for LRC lyrics
- Support for multiple local music libraries
- Support for multiple lyrics providers, including:
  - [LRCLIB](https://lrclib.net)
  - [SimpMusic](https://www.simpmusic.org)
  - [Genius (plain only)](https://genius.com)
  - [AZLyrics (plain only)](https://azlyrics.com)

### Lyric management

- Embed *existing* sidecar `.lrc` or `.txt` files into lyrics tags
- Remove existing lyrics files if a file is already tagged
- Convert LRC lyrics to plain lyrics

## Screenshots

| Library | Library (dark mode) |
|---|---|
| ![Library](https://raw.githubusercontent.com/weiteck/lyricade/main/data/screenshots/1.png) | ![Library (dark mode)](https://raw.githubusercontent.com/weiteck/lyricade/main/data/screenshots/2.png) |

| Preferences | Lyrics viewer |
|---|---|
| ![Preferences](https://raw.githubusercontent.com/weiteck/lyricade/main/data/screenshots/3.png) | ![Lyrics viewer](https://raw.githubusercontent.com/weiteck/lyricade/main/data/screenshots/4.png) |

## Supported Audio File Formats

**Lyricade** uses [`lofty-rs`](https://github.com/Serial-ATA/lofty-rs) for reading and writing audio file metadata tags. Please refer to their repo for supported formats.

## Releases

The latest release is available as a Flatpak package via [Flathub](https://flathub.org/en/apps/io.github.weiteck.Lyricade), which comes pre-configured in many distribution software centres.

If you already have Flathub [configured](https://flathub.org/en/setup), you can install by running:

```bash
flatpak install flathub io.github.weiteck.Lyricade
```

An AppImage package is also provided for convenience, however Flatpak is the only recommended installation method. Because Lyricade utilises modern GTK4 features, you will likely not have the required up-to-date dependencies if you're not running the latest major release of your Linux distribution.

## Building

**Lyricade** is intended to be packaged as a Flatpak application. The Flatpak can be built and installed locally by following the below steps.

### Requirements

- git
- Flatpak
- flatpak-builder
- GNOME Platform and SDK
- Rust SDK extension

#### 1. Clone the repository

```bash
git clone https://github.com/weiteck/lyricade.git && cd lyricade
```

#### 2. Install the required runtimes

```bash
flatpak install flathub \
    org.gnome.Platform//50 \
    org.gnome.Sdk//50 \
    org.freedesktop.Sdk.Extension.rust-stable//25.08
```

(Ensure the runtime version matches that specified by the manifest file [io.github.weiteck.Lyricade.dev.yml](io.github.weiteck.Lyricade.dev.yml))

#### 3. Build

```bash
flatpak-builder \
    --user \
    --install \
    --force-clean \
    build-dir \
    io.github.weiteck.Lyricade.dev.yml
```

#### 4. Run

```bash
flatpak run io.github.weiteck.Lyricade
```

## Project Status

Lyricade is under active development. Features, file formats and user interface elements may change between releases.

Bug reports, feature requests and pull requests are welcome.

## Contributing

AI-generated code or pull requests will be rejected.

Please ensure code is formatted consistently with the existing codebase before opening a pull request.

## License

Lyricade is licensed under Apache License 2.0. See [LICENSE](LICENSE) for details.
