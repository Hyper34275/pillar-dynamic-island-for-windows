# Yuval brand assets

The chosen mark (icon 2) and the chosen wordmark (B). The raster files in the product are generated from
these SVGs by `node scripts/make-brand-assets.cjs`; never edit the generated files by hand.

All SVGs are plain vector paths: no `<text>`, no filters, no raster, no external fonts, no Windows system font.

## Icon

A deep ink circle, an iridescent aurora ring (cyan, indigo, violet, magenta, pink: the island's own rim) and a
glowing Y. The Y is the capital Y of wordmark B, fitted into the circle.

| File | Use |
|---|---|
| `icon-2.svg` | 64 px and larger (1024 viewBox, transparent background) |
| `icon-2-small.svg` | 16, 20, 24, 32, 40 and 48 px: a bolder hand-built Y (a long right arm with a hooked tail and a short left arm, near-white core), a brighter and slightly thinner ring, no hairlines |

Checked on `#202020` and `#F3F3F3` taskbars at 16, 24, 32 and 48 px.

## Wordmark

The name "Yuval" in a connected brush signature.

| File | Colour | Use |
|---|---|---|
| `wordmark-b.svg` | aurora gradient | readable on dark and on light |
| `wordmark-b-dark.svg` | white `#FFFFFF` | dark backgrounds |
| `wordmark-b-light.svg` | near-black `#14141C` | light backgrounds |

Suggested minimum height: 28 px.

### Source font and licence

The outlines come from **Mr Dafoe** by Alejandro Paul (Sudtipos): "Copyright (c) 2011 Alejandro Paul
(sudtipos@sudtipos.com), with Reserved Font Name "Mr Dafoe"". Licence: SIL Open Font License 1.1, the text is in
`licenses/MrDafoe-OFL.txt`. The font file came from `github.com/google/fonts` (`ofl/mrdafoe/MrDafoe-Regular.ttf`).

- The OFL allows using a font to make artwork (a logo) and shipping that artwork. Only vector outlines are shipped,
  never the font file, so the OFL's conditions for distributing a font are not triggered.
- The font declares a Reserved Font Name: do not name any product, file or font "Mr Dafoe". The wordmark is an
  outline drawing of one word, not a font.
- Third-party notice: `THIRD_PARTY_NOTICES.md`.

### How the outlines were made

1. The word was shaped with HarfBuzz (so the font's contextual forms and kerning apply) and each glyph converted to
   SVG path data, scaled to 1000 units per em.
2. The raw font leaves two gaps. The "uval" group was moved 78 units left and 18 up so the Y flows into the u, and
   "al" a further 18 left and 12 up so the v joins the a. After that the word is exactly one connected shape
   (checked by rasterising and counting connected components).
3. The top of the "a" had a small double bump (the font's brush texture); it was replaced by one clean rounded
   terminal.
4. The paths are baked (no transforms) into one `<path>` per file, coordinates rounded to 0.1 unit.

## Where the generated files go

`node scripts/make-brand-assets.cjs` writes:

- `src-tauri/icons/icon.ico`: 16, 20, 24, 32, 40, 48 from `icon-2-small.svg`; 64, 128, 256 from `icon-2.svg`.
  It is the exe icon of Yuval.exe, the installer icon and the tray icon (Tauri takes the tray image from the exe's
  icon resource at the system icon size, 32 px at 100% scale, so the tray gets the small drawing).
- `src-tauri/icons/icon.png`: 512 px.
- `center/CompanyIsland.Center/Assets/icon.ico`: the same .ico, for Yuval.Center.exe and the Center's window icon.
- `src-tauri/nsis/installer-header.bmp` (150 x 57, light, wordmark) and `installer-sidebar.bmp` (164 x 314, dark
  ink, icon and wordmark): the installer's header and welcome/finish pages (`headerImage` and `sidebarImage` in
  `src-tauri/tauri.installer.conf.json`).

In the web code the same drawings exist as `src/components/brand/YuvalMark.tsx` and `YuvalWordmark.tsx`
(geometry in `paths.ts`, kept in sync with these SVGs by `brandSource.test.ts`).
