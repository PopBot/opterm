# Terminal font fallbacks

Opterm uses the chosen monospace face, installed outline fonts for available CJK characters, and a bundled monochrome emoji fallback. The bundled font supplies modern emoji outline glyphs that the renderer's older default emoji font lacks. It does not add color emoji or complete Unicode shaping/sequence support.

## Bundled Noto Emoji

- Asset: `assets/fonts/NotoEmoji-wght.ttf` (1,982,596 bytes), copied without modification from Google's official [Google Fonts repository](https://github.com/google/fonts/tree/b979dba422e445492b0eb9951ac52ee0b4d648c3/ofl/notoemoji).
- Source commit: `b979dba422e445492b0eb9951ac52ee0b4d648c3` (Noto Emoji 3.002 update).
- Original filename: `NotoEmoji[wght].ttf`; [pinned download](https://raw.githubusercontent.com/google/fonts/b979dba422e445492b0eb9951ac52ee0b4d648c3/ofl/notoemoji/NotoEmoji%5Bwght%5D.ttf).
- Font SHA-256: `de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551`.
- License: SIL Open Font License 1.1; copyright Google LLC. The complete notice and license accompany the font in `assets/fonts/NotoEmoji-OFL.txt`, copied from the [same source commit](https://raw.githubusercontent.com/google/fonts/b979dba422e445492b0eb9951ac52ee0b4d648c3/ofl/notoemoji/OFL.txt).
- License SHA-256: `500bb1ccf43df7bbb522112f9133a52b16e1c35e809632f5d8609b179152de5b`.

The font contains TrueType `glyf` outline data and a variable weight axis. Opterm uses its default instance through egui/ab_glyph; no font transformer, download at runtime, or executable font plugin is involved. The renderer test verifies that `🙂`, `🚀`, and `🧑` exist and generate nonempty rasterized glyphs using this asset alone.

CJK availability still depends on installed outline fonts such as Noto CJK, PingFang or Microsoft YaHei. Bitmap color-emoji fonts are not a substitute for supported outline fallback fonts. Emoji presentation, joined sequences, combining marks and script shaping need their own acceptance checks; individual glyph coverage does not prove all of them.
