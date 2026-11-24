# pivt

A sleek TUI cheatsheet for CTF / pentest commands (Rust + ratatui).

## Discord Rich Presence

pivt can advertise what you're working on to Discord via Rich Presence:

- **details** — the active room name + flags captured (e.g. `DC-1 · 3 flags`),
  or `browsing commands` when no room is active.
- **state** — the current target with reachability and the box timer
  (e.g. `◎ 10.10.11.42 up · ⏱ 12:34`).
- an **elapsed timer** driven by the box start time.
- a large image asset keyed `pivt` with the text `pivt — ctf toolkit`.

It is **opt-in** and a silent no-op when Discord isn't running — the connection
lives on a background thread and never blocks or crashes the UI.

### Enabling it

1. Create a Discord application at
   <https://discord.com/developers/applications> and copy its **Application ID**.
2. In **Rich Presence → Art Assets**, upload an image asset named **`pivt`**
   (this is the large image shown in the presence).
3. Put the Application ID into `~/.config/pivt/config.toml`:

   ```toml
   discord_rpc = true
   discord_app_id = "123456789012345678"
   ```

   You can also flip `discord_rpc` from inside the app by pressing **`D`**
   (Shift-D) in Normal mode. The setting is persisted. If you toggle it on
   without an `discord_app_id` set, pivt tells you to add one.

The presence refreshes about every 5 seconds while pivt runs.
