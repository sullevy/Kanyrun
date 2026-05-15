# Kanyrun Resident Daemon Design

## Goal
- Reduce repeated hotkey latency.
- Keep memory usage low when idle.
- Preserve `menu.ini` / `menu.conf` compatibility.
- Hide extension-gated menus unless the current file context matches.
- Fall back to a default callback menu when a matched menu becomes empty after filtering.

## Process Model
- `kanyrun` becomes the client entrypoint by default.
- The client connects to a resident daemon over a Unix socket under `XDG_RUNTIME_DIR`.
- If the daemon is missing, the client spawns it automatically.
- The daemon caches the resolved config and owns a warm `kanyrun-ui` child.
- The warm UI child is reused across multiple menu requests over framed stdin/stdout.

## Idle Policy
- `--ui-idle-timeout <sec>` controls when the warm UI child is reaped.
- `--daemon-idle-timeout <sec>` controls when the daemon exits after inactivity.
- `--oneshot` bypasses the daemon and uses the old single-shot flow.

## Menu Rules
- `MenuDefinition.extensions` from `menu.ini` are now enforced at render time.
- Submenus tagged like `-Python::python|py` only appear for matching file extensions.
- `[menus].fallback_id` is used when no rule matches or when a selected menu becomes empty after extension filtering.

## Verification
- Rust unit tests cover daemon protocol, rule fallback, and extension-gated submenu filtering.
- Existing one-shot behavior remains available through `--oneshot`.
