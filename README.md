<p align="center">
  <img src="https://raw.githubusercontent.com/weiteck/lyricade/refs/heads/main/data/icons/io.github.weiteck.Lyricade.svg" width="128" alt="Lyricade icon">
</p>

# Lyricade

A Linux desktop application for fetching and managing lyrics in your local music library. It will scan your music and download missing lyrics, in either synchronised LRC or plain text format, at your preference.

**Lyricade** is a modern GTK4/libadwaita application that aims to follow GNOME Human Interface Guidelines.

## Features
      
- Scan your music and find missing lyrics from [lrclib.net](https://lrclib.net)
- Download either synchronous LRC or plain lyrics
- Embed lyrics to the metadata tag, save as a sidecar file, or both
- Supports multiple local music libraries

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

View the [latest](https://github.com/weiteck/lyricade/releases/latest) release to download an `AppImage` package, or see the next section to build and install as a `flatpak`.

## Building

Lyricade is primarily developed and packaged as a `flatpak`. The `flatpak` can be built and installed locally by following the below steps.

### Requirements

- git
- Flatpak
- flatpak-builder
- GNOME Platform and SDK matching the manifest

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

(Build with the runtime version specified by the project's Flatpak manifest file `io.github.weiteck.Lyricade.yml`)

#### 3. Build

```bash
flatpak-builder \
    --user \
    --install \
    --force-clean \
    build-dir \
    io.github.weiteck.Lyricade.yml
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
