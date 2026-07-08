# Security Policy

## What EasyQuickScreenshot does (and doesn't) do

EasyQuickScreenshot is a local, single-user Windows utility. By design:

- **No network.** It never connects to the internet, sends telemetry, or phones home.
- **No accounts, no cloud.** Everything stays on your machine.
- **Your screenshots stay local.** They are written only to the folder you configure. The
  repository's `.gitignore` blocks `shots/` and all image files so captures can never be
  committed by accident.
- **No secrets.** The app stores no credentials or tokens; `config.toml` holds only hotkeys and
  folder paths.

The only OS integrations it uses are: global hotkeys, GDI screen capture, the clipboard, an
optional `HKCU\...\Run` autostart entry (opt-in, via `scripts/autostart.ps1`), and opening your
save folder / config file in Explorer.

## Supported versions

Only the latest release receives security fixes.

| Version | Supported |
|---|---|
| latest | ✅ |
| older | ❌ |

## Reporting a vulnerability

Please report security issues **privately** — do not open a public issue for anything
exploitable.

- Preferred: use GitHub's **[Report a vulnerability](https://github.com/david-vrba/EasyQuickScreenshot/security/advisories/new)**
  (Security → Advisories) to open a private advisory.

Include: what you found, steps to reproduce, affected version, and impact. I aim to acknowledge
within a few days. Because this app has no server component, the realistic surface is local
(e.g. handling of a crafted `config.toml` or a malicious image dropped into the saved folder) —
those reports are still welcome.

Thank you for helping keep the project safe.
