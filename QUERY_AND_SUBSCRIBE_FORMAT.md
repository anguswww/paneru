# Query and Subscribe Format

Paneru exposes structured state over the same IPC channel used by `send-cmd`:
a Mach service named `com.github.karinushka.paneru`. The CLI commands below
require a running Paneru daemon.

**The JSON below is what the CLI prints, not what crosses between processes.**
Requests and responses travel as typed values in a compact binary encoding
(`postcard`); `paneru query` and `paneru subscribe` render them as JSON because
a terminal — and `jq`, and a status bar's shell script — needs text. Anything
consuming these commands' output sees exactly the shapes documented here.

A client written in Rust can skip the JSON entirely by using the
`paneru::types` module (`src/types`): its `wire::Request` and `wire::Response`
are the protocol, and `async-mach-ports` is the transport.

When Paneru is installed as a service, its Mach service name belongs to launchd,
and reaching it starts the service if it is stopped. Pass `--no-launch` (before
the subcommand, e.g. `paneru --no-launch query state --json`) to have a client
command fail with `paneru is not running` instead. It checks a second name,
`com.github.karinushka.paneru.running`, that the daemon registers itself and
that therefore only resolves while it is alive; a daemon too old to register it
is reported as not running.

All query responses are a single JSON document. `subscribe` emits
line-delimited JSON, with one complete event object per line.

## Query Commands

```shell
paneru query state --json
paneru query virtual-workspaces --json
paneru query active --json
```

`--json` is accepted for clarity and is the only output format, so it may be
omitted; callers should include it anyway, in case another format is ever added.

### `paneru query state --json`

Returns the complete state document.

```json
{
  "version": 1,
  "timestamp": 1777740000,
  "active": {
    "display_id": 1,
    "native_workspace_id": 4,
    "virtual_workspace_number": 3,
    "focused_window_id": 321,
    "focused_bundle_id": "com.apple.Terminal",
    "focused_app_name": "Terminal",
    "focused_window_title": "paneru"
  },
  "virtual_workspaces": [
    {
      "number": 1,
      "native_workspace_id": 4,
      "active": false,
      "windows": []
    },
    {
      "number": 2,
      "native_workspace_id": 4,
      "active": false,
      "windows": []
    },
    {
      "number": 3,
      "native_workspace_id": 4,
      "active": true,
      "windows": [
        {
          "window_id": 321,
          "bundle_id": "com.apple.Terminal",
          "app_name": "Terminal",
          "title": "paneru",
          "role": "AXWindow",
          "subrole": "AXStandardWindow",
          "focused": true,
          "floating": false,
          "tab_group": null,
          "cell": 321,
          "cell_front": true
        }
      ]
    }
  ],
  "displays": [
    {
      "display_id": 1,
      "frame": { "x": 0, "y": 0, "width": 1512, "height": 982 },
      "active": true,
      "native_workspace_id": 4,
      "virtual_workspace_number": 3
    }
  ]
}
```

### `paneru query virtual-workspaces --json`

Returns only the `virtual_workspaces` array from the complete state document.

```json
[
  {
    "number": 1,
    "native_workspace_id": 4,
    "active": false,
    "windows": []
  },
  {
    "number": 2,
    "native_workspace_id": 4,
    "active": false,
    "windows": []
  },
  {
    "number": 3,
    "native_workspace_id": 4,
    "active": true,
    "windows": [
      {
        "window_id": 321,
        "bundle_id": "com.apple.Terminal",
        "app_name": "Terminal",
        "title": "paneru",
        "role": "AXWindow",
        "subrole": "AXStandardWindow",
        "focused": true,
        "floating": false
      }
    ]
  }
]
```

### `paneru query active --json`

Returns only the active display, workspace, and focused-window state.

```json
{
  "display_id": 1,
  "native_workspace_id": 4,
  "virtual_workspace_number": 3,
  "focused_window_id": 321,
  "focused_bundle_id": "com.apple.Terminal",
  "focused_app_name": "Terminal",
  "focused_window_title": "paneru"
}
```

## Fields

| Field | Type | Description |
| :--- | :--- | :--- |
| `version` | number | State document format version. Currently `1`. |
| `timestamp` | number | Unix timestamp in seconds when the response was built. |
| `active` | object | Current active display/native workspace/virtual workspace/focused window. |
| `display_id` | number or null | CoreGraphics display id for the active display, when known. |
| `native_workspace_id` | number or null | macOS Space id for the active native workspace, when known. |
| `virtual_workspace_number` | number or null | One-based Paneru virtual workspace number, when known. |
| `focused_window_id` | number or null | Focused window id, when known. |
| `focused_bundle_id` | string or null | Bundle id of the focused window's app, when known. |
| `focused_app_name` | string or null | Display name of the focused window's app, when known. |
| `focused_window_title` | string or null | Title of the focused window, when known. |
| `virtual_workspaces` | array | Virtual workspace rows known to Paneru. |
| `number` | number | One-based virtual workspace number. |
| `active` | boolean | Whether this virtual workspace is currently selected. |
| `windows` | array | Managed windows in this virtual workspace row. |
| `window_id` | number | Window id. |
| `bundle_id` | string | Bundle id for the owning application, or an empty string if unknown. |
| `app_name` | string | Display name for the owning application, or an empty string if unknown. |
| `title` | string | Window title, or an empty string if unknown. |
| `role` | string or null | Accessibility role (e.g. `AXWindow`), or null if the window reports none. |
| `subrole` | string or null | Accessibility subrole (e.g. `AXStandardWindow`, `AXDialog`, `AXFloatingWindow`), or null if the window reports none. |
| `focused` | boolean | Whether this window is focused. |
| `floating` | boolean | Whether this window is unmanaged/floating. |
| `column` | number or null | Zero-based column of a tiled window in its strip, left to right; `null` for floating windows. |
| `row` | number or null | Zero-based row of a tiled window within its column, top to bottom; windows tabbed together share a row. |
| `tab_group` | number or null | Native (OS) tab group the window belongs to, named by the smallest window id among its tabs so it does not change as tabs are switched; `null` when the window is not a native tab. |
| `cell` | number or null | Layout cell — one frame on screen — the window occupies, named by the smallest window id in it. Native tabs share a cell, and so do all windows of a stack shown as tabs (`tabbed`). Group windows by `cell` to list each on-screen slot once. Floating windows have a cell of their own. |
| `cell_front` | boolean | Whether this is the window its cell is showing: the front native tab, the visible tab of a tabbed stack, or the only window in the cell. |
| `displays` | array | Every connected display, ordered left to right. |
| `displays[].display_id` | number | CoreGraphics display id. |
| `displays[].frame` | object | The whole display (menu bar included) in global coordinates: `x`, `y`, `width`, `height`. |
| `displays[].active` | boolean | Whether this display has the active menu bar. |
| `displays[].native_workspace_id` | number or null | macOS Space currently shown on this display, when known. |
| `displays[].virtual_workspace_number` | number or null | One-based virtual workspace row shown on that Space, when Paneru manages it. |

Paneru may include empty `windows` arrays for missing virtual workspace numbers
inside a native workspace so integrations can render stable numbered slots.

## Subscribe Command

```shell
paneru subscribe --json
```

`subscribe` keeps its channel open and writes one JSON event per line. The stream
is intended for integrations such as SketchyBar, so it emits changes that are
useful for keeping a bar in sync: focus changes, native or virtual workspace
changes, managed window-list changes, window title changes, and display changes.
Paneru coalesces duplicate internal events from the same ECS tick and skips
events whose relevant state has not changed since the last emitted event.
Consumers should parse each line independently and then call
`paneru query state --json` when they need a full refresh — or subscribe with
`--state` and never query at all.

### State snapshots

```shell
paneru subscribe --json --state
```

With `--state`, the stream also carries the complete state document — exactly
what `paneru query state --json` prints — as a `snapshot` event:

```json
{"event":"snapshot","state":{"version":1,"timestamp":1777740000,"active":{...},"virtual_workspaces":[...],"displays":[...]}}
```

One snapshot is sent as soon as the subscription starts, and another after every
batch of events that changed the document (its `timestamp` aside); it follows
that batch's other events. A client that only renders the latest state can
ignore every other event and keep the most recent snapshot.

### Event Types

```json
{"event":"virtual_workspace_changed","active":{"display_id":1,"native_workspace_id":4,"virtual_workspace_number":3,"focused_window_id":321,"focused_bundle_id":"com.apple.Terminal","focused_app_name":"Terminal","focused_window_title":"paneru"}}
```

Emitted after native Space changes and Paneru virtual workspace switches. Paneru
derives this from both incoming workspace events and ECS active-workspace marker
changes, so integrations receive the event when the visible workspace state
changes.

```json
{"event":"windows_changed","virtual_workspace_number":3,"active":{"display_id":1,"native_workspace_id":4,"virtual_workspace_number":3,"focused_window_id":321,"focused_bundle_id":"com.apple.Terminal","focused_app_name":"Terminal","focused_window_title":"paneru"}}
```

Emitted after managed window creation/destruction/minimize/deminimize events and
after Paneru moves or sends a window between virtual workspaces. The event is
emitted only when Paneru's virtual workspace/window state differs from the last
emitted `windows_changed` event.

```json
{"event":"window_focused","window_id":321,"bundle_id":"com.apple.Terminal","title":"paneru","virtual_workspace_number":3}
```

Emitted when focus changes. Paneru derives this from both incoming focus events
and ECS focused-window marker changes, so internally handled focus transitions
are visible to subscribers. The `window_id`, `bundle_id`, `title`, and
`virtual_workspace_number` fields are taken from the final active state for the
tick, so stale lower-level focus notifications are not forwarded with mismatched
window metadata.

```json
{"event":"window_title_changed","window_id":321,"title":"paneru"}
```

Emitted when a window title changes.

```json
{"event":"display_changed","display_id":1}
```

Emitted when display configuration changes. `display_id` can be `null` when the
event is a global display-change notification and Paneru cannot resolve an
active display id.

## Virtual Workspace Commands

Absolute virtual workspace selection is addressed as a window command:

```shell
paneru send-cmd window virtualnum 3
paneru send-cmd window virtualmovenum 3
paneru send-cmd window virtualsendnum 3
```

`virtualnum` can name the display whose Space it switches, using a
`display_id` from the state document's `displays` array:

```shell
paneru send-cmd window virtualnum 3 display 2
```

On the active display this is a plain `virtualnum 3`. On another display, Paneru
first activates that display the way `paneru send-cmd mouse display 2` does, then
selects the row once macOS reports the display active. If the display does not
become active within a second (one Paneru cannot focus a window on, say), or
another virtual workspace command arrives first, the switch is dropped.

The matching config binding names are:

```toml
[bindings]
window_virtualnum_3 = "cmd + alt - 3"
window_virtualmovenum_3 = "cmd + alt + ctrl - 3"
window_virtualsendnum_3 = "cmd + alt + shift - 3"
window_virtualnum_3_display_2 = "cmd + alt + ctrl + shift - 3"
```
