# Screen Recorder (nc-screen-recorder)

Noctalia v5 plugin for screen recording and streaming, powered by
[gpu-screen-recorder](https://github.com/dec05eba/gpu-screen-recorder) with
[slurp](https://github.com/emersion/slurp)-based region selection.

- **ID**: `bighu630/nc-screen-recorder`
- **Version**: 2.0.0 (plugin API 26)
- **License**: MIT
- **Tags**: bar, panel, utility

## Dependencies

- `gpu-screen-recorder` — actual recording/streaming backend
- `slurp` — interactive region selection (`slurp -f "%wx%h+%x+%y"`, output is
  fed directly to gpu-screen-recorder's `-region WxH+X+Y`, no coordinate
  conversion needed)

## Architecture

The plugin is split into three entries that communicate through the state
channel (`rec_state` / `rec_options` / `rec_cmd`, JSON-string payloads):

| Entry | File | Responsibility |
|---|---|---|
| `[[service]] recorder_service` | `service.luau` | Owns the recorder process and the state machine; sole writer of state. Builds gpu-screen-recorder commands, handles slurp region selection, pidfile/session-based stop and restart recovery, and persists option overrides. |
| `[[widget]] recorder` | `widget.luau` | Status-bar capsule: idle glyph in normal color, static red dot + elapsed timer + mic glyph while recording. Left click toggles the panel, right click opens plugin settings. |
| `[[panel]] main` | `panel.luau` | Operation UI anchored below the widget (380×600, attached): status row, start/stop button, six dropdowns (audio source, target, codec, quality, framerate, stream destination), settings entry. |

Recording lifecycle: start resolves the target (`region` → slurp, Esc/empty →
cancelled toast; `fullscreen` → `-w screen`; otherwise `-w <monitor>`), spawns a
wrapper script via `runStream` that records the pid to `recorder.pid`. Stop sends
`SIGINT` through the pidfile. Exit code `0`/`255` counts as success (file saved /
stream stopped), anything else raises an error notification. On service startup,
a surviving pid in `session.json` resumes the recording state; otherwise state is
cleaned up back to idle.

## Settings

| Key | Type | Default | Description |
|---|---|---|---|
| `saveDirectory` | folder | `~/Videos` | Folder where recordings are saved |
| `filePattern` | string | `recording_{datetime}` | Output file name pattern; `{datetime}` expands to a timestamp |
| `videoFormat` | select (`mp4`/`mkv`/`webm`) | `mp4` | Container format |
| `codec` | select (`auto`/`h264`/`hevc`/`av1`/`vp9`) | `auto` | Video codec |
| `quality` | select (`medium`/`high`/`very_high`/`ultra`) | `high` | Quality preset |
| `framerate` | select (`30`/`60`/`120`/`144`) | `60` | Recording FPS |
| `audioSource` | select (`none`/`mic`/`desktop`/`both`) | `none` | Audio track to record |
| `streamDestinations` | string_map | `{}` | Named RTMP/RTSP streaming destinations |
| `streamDestination` | string | `""` | Destination used in streaming mode |

Changes made inside the panel are applied immediately by writing overrides via
the service; declarative settings act as defaults.

## Manual verification checklist (after install)

1. Capsule shows both states correctly: idle glyph normally, red dot + timer
   (+ mic icon when audio is on) while recording.
2. Left click opens the panel attached right below the widget icon.
3. Region recording produces a valid video file (select "Region (slurp)",
   drag a rectangle, record, stop).
4. Stopping with SIGINT ends gracefully — exit code 0/255 — and the file is
   saved with a success notification.
5. After restarting (host or plugin reload) no stale recording state remains:
   if the old process is dead the widget returns to idle.
