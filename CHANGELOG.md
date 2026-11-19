# Changelog

All notable changes to Litecord are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-11-19

The first release under the Litecord name.

### Changed

- Renamed the project from Yapfast to Litecord, including the executable,
  window title, protocol handler, installer, desktop entry, AppStream ID and
  data directory.
- New monochrome app icon: a white chat bubble and bolt on a black tile.
- The Lite theme now uses a black-and-white accent instead of marigold:
  selection, primary actions, unread dividers and mentions are monochrome,
  and the tray icon matches the app icon.
- Updates now come from this repository, and About links to this project's
  issues instead of the retired website and Discord server.
- Credits now name the Litecord maintainer, with the original Yapfast
  attribution preserved in `NOTICE`.

### Added

- Apple's emoji artwork, packed into the embedded colour font, so every
  supported emoji renders as Apple's own picture instead of Twemoji.
- Issue templates, a pull request template and a security policy.

### Fixed

- The emoji picker and message text share one font, so an emoji looks the same
  everywhere it appears.

[0.5.0]: https://github.com/ganddtn40/litecord/releases/tag/v0.5.0
