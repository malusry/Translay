# Translay desktop icon V3

Transparent-background desktop icon candidate. This directory is intentionally
not referenced by Tauri or the application runtime yet.

## Design decisions

- Shorter top bar and wider stem for a more stable desktop silhouette.
- Darker sage-to-indigo gradient for mixed wallpaper contrast.
- Recessed translation groove instead of a transparent cut-out.
- Size-specific optical SVGs for small Windows surfaces.
- No plate, tile, or opaque background.

## Sources

- `translay-desktop-v3-master.svg`: master artwork for 128 px and larger.
- `optical/`: hand-tuned sources for 16, 20, 24, 32, 48, and 64 px.

## Exports

- `png/`: raster exports from 16 through 512 px.
- `translay-desktop-v3.ico`: multi-resolution Windows icon candidate.
- `qa/translay-desktop-v3-contact-sheet.png`: light, mixed, dark, and
  transparency checks.

No files under `src-tauri/icons/` are changed by this design package.

## Re-export

Run `export-assets.mjs` with the path to an installed `sharp` package:

```text
node export-assets.mjs <absolute-path-to-sharp-package>
```

The exporter rebuilds the PNG, ICO, and QA contact-sheet outputs and validates
canvas size plus transparent corners.
