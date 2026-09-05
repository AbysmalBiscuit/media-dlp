# media-dlp design

Date: 2026-09-05

Status: approved for planning

## Purpose

A desktop application that wraps yt-dlp behind a small, obvious interface. Paste a link, pick a folder, press a button, get a file. Everything the underlying tool can do beyond that stays out of the way.

The interface is the product. yt-dlp already downloads anything; what this project adds is that using it requires no terminal, no flags, and no decisions beyond four radio groups and a checkbox.

## Verified facts this design rests on

Every claim below was read out of the yt-dlp checkout, not recalled. Paths are relative to the yt-dlp repository at `master`.

- Chromium cookie extraction is broken on Windows. `_get_windows_v10_key` (`yt_dlp/cookies.py:1013`) reads only `os_crypt.encrypted_key` from Chrome's Local State and unwraps it with DPAPI. The file contains no handling for `app_bound_encrypted_key` and no `v20` branch. Chrome 127 and later re-encrypt cookies under App-Bound Encryption with a `v20` prefix, which falls through to the raw DPAPI path and fails at `yt_dlp/cookies.py:1099`. Firefox is unaffected: its cookie database is plain SQLite with no key unwrapping. This was confirmed by reading the source, not by testing a live profile.
- The official Windows x64 `yt-dlp.exe` is built from `bundle/requirements/curl-cffi.txt` (`.github/workflows/build.yml:464`), which is the `default` extras plus `curl-cffi`. Those are every extra that applies on Windows; `secretstorage` is Linux-only and `deno` is an optional JavaScript runtime. Shipping the official binary gets the full-featured build with no custom packaging.
- `-S res:720` selects the largest format no larger than 720p, falling back to the smallest available when nothing fits (`README.md:1641`). A `[height<=720]` filter fails outright in that case, so the sort is used for quality caps.
- `--remux-video` fails when the target container cannot hold the codec (`README.md:953-961`). Container choice is therefore steered through format sorting and `--merge-output-format` (`README.md:885-888`) instead.
- `--audio-quality` runs 0 (best) to 10 (worst) for VBR, or an explicit bitrate, and defaults to 5 (`README.md:949-952`). It applies only when `-x` converts.
- The conditional-prefix output template idiom is documented: `%(playlist_index&{} - |)s` emits the field followed by a literal when the field is present and nothing at all when it is absent (`README.md:1491`).
- Unavailable output template fields render as the literal `NA` by default (`README.md:666`).
- `--progress-template` exposes the progress dictionary under a `progress` key and the video's fields under `info` (`README.md:808`), throttled by `--progress-delta` (`README.md:818`). The progress dictionary's fields are documented at `yt_dlp/YoutubeDL.py:397-421`: `status` (one of `downloading`, `error`, `finished`), `filename`, `tmpfilename`, `downloaded_bytes`, `total_bytes`, `total_bytes_estimate`, `elapsed`, `eta`, `speed`, `fragment_index`, `fragment_count`.
- `-J` dumps whole-playlist information as a single JSON line (`README.md:795-799`), and `playlist_count` carries the item total (`README.md:1374`).
- yt-dlp offers three update channels, `stable`, `nightly` and `master` (`yt_dlp/update.py:38`). Stable's cadence is uneven: the changelog shows 2026.03.17 followed by no release until 2026.06.09.

## Decisions

| Decision | Choice | Reason |
| --- | --- | --- |
| Shell | Tauri | Native window, native folder picker and a real installer, none of which a compiled Bun or Node binary provides |
| Frontend | Astro, built with Bun, styled with Tailwind | Static output, no framework runtime needed for a single screen |
| Backend | Rust, four small modules | Subprocess control, file I/O and settings; the webview cannot spawn processes |
| Platforms | Windows first | Linux and macOS come later via CI, so nothing in the design may assume Windows |
| Binaries | yt-dlp and ffmpeg bundled in the installer | Works offline on first launch, no first-run fetch to get wrong |
| yt-dlp updates | Self-update on the `nightly` channel, checked in the background at launch | Extractor breakage outpaces the stable release cadence |
| Downloads | One URL at a time, playlists supported | A pasted playlist link is a normal thing to do and must not dead-end |
| Cookies | Chrome and Firefox both shown, Chrome disabled where unsupported | Hiding the option invites the question; a disabled control with an explanation answers it |
| Config | `config.toml`, serde | Human-readable and hand-editable |

## Architecture

Three layers.

1. Astro builds a static bundle that Tauri serves into the system webview.
2. A Rust core exposing Tauri commands and emitting typed events.
3. Two bundled external binaries, yt-dlp and ffmpeg.

The frontend never sees a yt-dlp flag. It sends settings and a URL; it receives typed events.

### Download flow

1. The user pastes a URL. The frontend debounces and calls the probe command.
2. `job` runs `yt-dlp -J --no-warnings -I 1 <url>`, which fully extracts the first item and reports `playlist_count` when the link is a playlist.
3. The frontend renders the title, the thumbnail, and a filename preview built from the real metadata.
4. The user presses Download. `args` turns settings plus metadata into a complete argv. `job` spawns the process.
5. Progress lines stream back as events; the bar and the "video 3 of 12" counter update.
6. A finished event names the output file and offers a button that opens its containing folder.

### Rust modules

Each module has one job and does not reach into another's internals.

**`settings`** owns `config.toml` in the Tauri application config directory. One serde-derived struct. Loading fills defaults for anything missing or malformed, so a hand-edited or truncated file degrades rather than crashes. Two commands, get and save.

**`binaries`** resolves where yt-dlp and ffmpeg live. Both ship as Tauri resources. On first run this module copies yt-dlp into the user-writable application data directory, because the install directory is not writable and `--update-to` overwrites the binary in place. ffmpeg stays in the read-only resource directory and is passed as `--ffmpeg-location`; it does not self-update. This module also runs the launch-time update check, which never blocks a download.

**`args`** is a pure function from settings, probe metadata and a URL to a complete argv. No I/O and no spawning. Every format radio, every quality tier and every filename chip resolves here. Being pure makes it the module worth testing exhaustively, and makes any question of the form "what does this combination actually run" answerable by an assertion on a string.

**`job`** spawns the process, reads stdout line by line, parses progress lines and emits events. It sets `CREATE_NO_WINDOW` on Windows so no console flashes. stderr is captured separately so failures can report a real reason. Cancelling kills the process and removes the partial file.

## Interface

### Main screen

Top to bottom:

1. URL field.
2. Save folder, with a Browse button opening the native folder picker. The URL and the folder are the two controls used on every download, so they sit together at the top.
3. Title and thumbnail, once the probe returns.
4. Audio only checkbox. Ticking it collapses the video radio groups rather than disabling them, so the screen gets shorter instead of showing dead controls.
5. Video quality radios: Best available, 1080p, 720p, 480p.
6. Video format radios: mp4, mkv, webm.
7. Audio format radios: mp3, m4a, opus, wav.
8. Audio quality radios: Best, Good, Smaller.
9. Filename chips with a live preview.
10. Download button, which becomes a progress bar with a Cancel button.

Every control on this screen persists its last value.

### Settings screen

Small, reachable from a single control on the main screen.

- Theme: System, Light, Dark.
- Default save folder.
- yt-dlp update channel, with nightly preselected.
- A Check for updates button showing the currently installed version.
- The path to `config.toml`, with a button that opens it.

### Theme

CSS custom properties. `prefers-color-scheme` supplies the System behaviour; an explicit override from `config.toml` is applied as a `data-theme` attribute on the root element so the choice wins in both directions.

## Format mapping

`<N>` is the selected quality cap. When Best available is selected, the `res:` term is omitted from the sort.

### Video, audio only unchecked

Every case uses `-f "bv*+ba/b"`.

| Format | Sort | Merge target |
| --- | --- | --- |
| mp4 | `-S "res:<N>,ext:mp4:m4a"` | `--merge-output-format mp4` |
| mkv | `-S "res:<N>"` | `--merge-output-format mkv` |
| webm | `-S "res:<N>,ext:webm:webm"` | `--merge-output-format webm` |

The `ext` sort term steers selection toward codecs the target container holds natively, so the merge is a remux rather than a re-encode. mkv accepts every codec and therefore needs no `ext` preference. `--remux-video` is deliberately not used, because it hard-fails on an incompatible codec instead of adapting.

### Audio, audio only checked

`-f "ba/b" -x --audio-format <format> --audio-quality <quality>`.

| Format | Typical result on YouTube |
| --- | --- |
| mp3 | Always re-encodes. The default, because it plays on every device. |
| m4a | Remuxes when the source is AAC. Fast and lossless. |
| opus | Remuxes when the source is Opus. Fast and lossless. |
| wav | Always re-encodes into a large lossless file. |

Quality maps Best to 0, Good to 5 and Smaller to 9.

The quality setting has no effect when the format remuxes. The interface states this in one line of helper text under the quality group rather than hiding it.

### Flags common to every download

- `-P "<save folder>"`
- `-o "<filename template>"`
- `--trim-filenames 120`, guarding the Windows path length limit (`README.md:674-676`)
- `--ffmpeg-location "<resolved ffmpeg path>"`
- `--newline`
- `--progress-delta 0.5`
- `--progress-template "download:<progress line>"`
- `--cookies-from-browser <browser>`, only when a browser is selected

## Filename builder

Four chips: Channel, Upload date, Playlist number, Title. Title is always on and cannot be unticked. A separator picker offers `" - "`, `"_"` and `" "`.

Chips compose in a fixed order, playlist number first and title last, into a single output template. The conditional-prefix idiom means one template serves both a lone video and a playlist item with no branching in our code: an absent field contributes nothing, not even its separator.

Template shape, with `<S>` standing for the chosen separator:

```
%(playlist_index&{}<S>|)s%(upload_date&{}<S>|)s%(uploader&{}<S>|)s%(title)s.%(ext)s
```

The preview renders against the probe metadata, so it shows the real filename.

Unverified: whether strftime formatting and conditional replacement compose in a single field, that is whether `%(upload_date>%Y-%m-%d&{}<S>|)s` parses. The documentation describes both features separately and never shows them combined. The implementation verifies this against the bundled binary and falls back to the plain form yielding `20260905` if they do not compose.

## Cookies

`--cookies-from-browser` supplies cookies for sites that require a signed-in session.

Both Chrome and Firefox appear as radios. Where a browser is unsupported on the current platform the radio renders disabled with helper text explaining why, rather than being hidden. On Windows this means Chrome is disabled and the text says Chrome locks its cookies, so Firefox must be used instead.

Support is reported by a Rust command returning per-browser availability for the running platform. It is a runtime value, not a compile-time constant, so adding Linux and macOS later flips the state without touching the frontend. Chromium cookie extraction works on Linux and macOS, so this is not hypothetical.

## Binaries and updates

Both binaries ship inside the installer as Tauri resources.

On first run, `binaries` copies yt-dlp into the user-writable application data directory, because `--update-to` overwrites the binary in place and the install directory is not writable by a normal user. Every later invocation uses that copy. ffmpeg is invoked from the read-only resource directory via `--ffmpeg-location`.

At launch, `binaries` checks for a yt-dlp update on the configured channel in the background. A check in flight never blocks a download; the running version is used and the update applies to the next launch.

## Errors

A non-zero exit shows one plain sentence and a collapsed "Show details" holding the tail of stderr with a Copy button, so a failure can be diagnosed from a screenshot without anyone opening a terminal.

One case is handled specifically: when stderr indicates a sign-in requirement, the message says so and points at the cookie browser control. That case is the entire reason cookie support exists, so it earns its own message. Everything else falls through to the generic sentence.

Cancelling kills the process and deletes the partial file, so a cancelled download leaves nothing behind.

## Progress parsing

The progress template emits one tab-delimited line per update, prefixed with a sentinel so lines yt-dlp writes for other reasons are ignored. The fields, in order:

`progress.status`, `progress.downloaded_bytes`, `progress.total_bytes`, `progress.total_bytes_estimate`, `progress.eta`, `progress.speed`, `progress.fragment_index`, `progress.fragment_count`, `info.playlist_index`, `info.playlist_count`, `info.title`.

Title comes last so a tab inside a title cannot corrupt the fields before it. The parser treats the literal `NA` as absent for every field.

## Configuration file

`config.toml` in the Tauri application config directory. One serde-derived struct holding save folder, video format, video quality, audio format, audio quality, audio-only, cookie browser, filename chip selections, separator, theme and update channel.

Missing and malformed values fall back to defaults rather than failing the load. The settings screen shows the file's path and opens it on request.

## Layout

```
src-tauri/    Cargo.toml, tauri.conf.json
              src/{main,settings,binaries,args,job}.rs
              binaries/  bundled yt-dlp and ffmpeg
ui/           Astro, Bun, Tailwind
docs/         this spec and the plan that follows it
devkit.toml   task definitions
```

Rust dependencies: `tauri`, `serde` with derive, `toml`, `tokio`, and the Tauri dialog and opener plugins for Browse and Open folder.

The subprocess runs through `tokio::process` rather than the shell plugin's sidecar mechanism, for two reasons: yt-dlp is copied to the application data directory for self-update and is no longer at a sidecar path, and the job module needs `CREATE_NO_WINDOW` and line-by-line stdout.

## Testing

`args` is the test surface that matters. It is pure, so every radio combination and every chip combination becomes a table-driven assertion on an exact argv. The test file doubles as the answer to any later question about what a given combination runs.

Two smaller suites:

- The progress line parser, against recorded sample lines including ones with `NA` fields and a tab inside a title.
- `settings`, covering a round trip plus a truncated and a malformed file both loading with defaults.

No test downloads a real video. Such a test would be slow, network-dependent, and would fail for reasons unrelated to this code. A manual smoke check is documented in the README instead.

## Out of scope

- Concurrent or queued downloads.
- Subtitle download, chapters, thumbnails as separate files, and metadata embedding.
- Format selection beyond the four radio groups.
- A download history or library view.
- Application self-update. Only yt-dlp self-updates.

## Resolved during design

- The Rust crate lives at `src-tauri/`. The `uv init` scaffold is gone; nothing needs a local interpreter, because the official yt-dlp binary is a PyInstaller build with its own embedded.
- Windows code signing is skipped. The installer is unsigned and SmartScreen warns on install. Revisit if the application is distributed beyond a handful of machines.
- ffmpeg ships as a full static build, roughly 80 MB and the bulk of the installer. A trimmed build would have to be produced and maintained, which is not worth the saving here.
