# media-dlp

A desktop wrapper around yt-dlp. Paste a link, pick a folder, press Download.

## Prerequisites

The commands below are devkit tasks, so devkit has to be installed to run them. It provides `devrun` and reads the task table in `devkit.toml`. Get it from [AbysmalBiscuit/devkit](https://github.com/AbysmalBiscuit/devkit).

## Running it

`devrun task dev` starts the Astro dev server and the Tauri window together.

## Tests

`devrun task check` runs formatting, clippy and the Rust suite. `devrun task ui-check` type-checks the frontend and `devrun task ui-test` runs its tests.

No test reaches the network. After changing anything in the argument builder or the progress parser, run the smoke check below by hand.

## Smoke check

1. `devrun task dev`
2. Paste a short public video link, choose a folder, press Download.
3. Confirm the file arrives with the name the preview showed.
4. Tick Audio only, repeat, confirm an audio file arrives.

## The bundled binaries

`src-tauri/binaries/` holds yt-dlp, ffmpeg and ffprobe. They are not committed. Fetch yt-dlp from its GitHub releases and ffmpeg and ffprobe from the same static build, then place all three there before building. yt-dlp is copied into the user's data directory at first run so it can update itself; ffmpeg and ffprobe are invoked from the install directory. yt-dlp resolves ffprobe from whatever directory holds ffmpeg, so the two must stay side by side.

The release workflow fetches all three itself. The versions it pins are in the `env` block of `.github/workflows/release.yml`, and bumping them there is the whole update.

## Installers

`devrun task build` produces an installer for whatever platform you are on, under `src-tauri/target/release/bundle/`.

Releases are cut by release-please. It keeps a release pull request open against `main`; merging it bumps the version, writes the changelog and tags the commit, and that tag starts the build that attaches installers for Windows, Apple Silicon and Intel macOS, and Linux.

Nothing is signed with a paid certificate, so every platform warns on first run:

- **Windows**: SmartScreen shows an unrecognised-app dialog. Choose More info, then Run anyway.
- **macOS**: the app is ad-hoc signed, which stops macOS calling it damaged, but it is not notarized. The first launch is refused; open System Settings, go to Privacy & Security, and press Open Anyway.
- **Linux**: no warning. The `.deb` and the AppImage both run as they are.
