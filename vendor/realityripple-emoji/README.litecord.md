# Apple Emoji for Litecord

This is the `apple` directory of **RealityRipple/emoji**, at commit
`db08071637157a9def981b59beae9a8d64e70778` (branch `trunk`, committed
2025-07-30). It is a plain copy of a sparse checkout: `apple/`, the
repository's licence files and its README are here, without Git history.

- **Source:** https://github.com/RealityRipple/emoji/tree/trunk/apple
- **Commit:** `db08071637157a9def981b59beae9a8d64e70778`
- **Files:** 3,773 PNGs, every one 112x112 RGBA
- **Names:** the emoji's Unicode code points in lower-case hexadecimal, joined
  by `-`, e.g. `1f600.png`, `1f468-1f3fd-200d-1f4bb.png`,
  `1f1ee-1f1e9.png`. Names with a variation selector include `-fe0f`.
- **Licence of the artwork:** `LICENSE-APPLE`, Copyright (C) Apple Inc., a
  BSD-style licence that requires the notice and disclaimer to be kept in
  source and binary distributions. The repository's own `LICENSE` is the
  Unlicense.

Litecord draws every supported emoji from these pictures rather than from a
system emoji font. `build.rs` reads each PNG, adds an 8 pixel transparent
margin, and packs the pictures into one CBDT/CBLC font that the program
carries; the picture's alphabetical place in the index is its Private Use Area
glyph. `src/emoji_pack.rs` finds a picture by its code-point name, and the
font's GSUB ligatures join skin tones, ZWJ sequences, flags, keycaps and
variation selectors into their one picture. No artwork is redrawn, recoloured
or replaced; the build only puts a lossless transparent margin around each
source PNG and re-encodes it losslessly.

## Updating

Replace `apple/`, the licence files and `REVISION` with the wanted commit's,
then rebuild. `REVISION` holds the commit for the package's dependency list.
When the folder has its own `.git`, `build.rs` reads its `HEAD` and refuses a
checkout whose `origin` is not `RealityRipple/emoji`; without one, it trusts
`REVISION`, so update that with the pictures. Keep `LICENSE-APPLE` beside
`apple/`, which the build checks for.
