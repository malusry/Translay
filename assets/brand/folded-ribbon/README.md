# Folded ribbon T

- `mark.svg`: approved full-gradient mark, shared with the selection button.
- `wordmark.svg`: folded T combined with the existing lettering; settings uses an external SVG URL to respect the app CSP.
- `tray-light.svg` / `tray-dark.svg`: approved small-size variants, simplified folds and strengthened strokes.
- `tray-collapsed-light.svg` / `tray-collapsed-dark.svg`: selected sideways fold for disabled selection icons. These retain the original paths and palette, splitting at the green/purple junction; green rotates -32°, purple +12°, both scale .96 around (32, 20). The enabled resources stay unchanged.
- `tools/render-brand.cjs`: generates Windows PNG/ICO and raw tray RGBA resources using sharp. Run from the repository with sharp available on NODE_PATH.

The ICO includes 16, 20, 24, 32, 40, 48, 64, 128 and 256px entries. Executable and installer use the same icon path. The tray reads Windows SystemUsesLightTheme and refreshes on changes (within approximately 3 seconds); failure to read the setting falls back to the dark-taskbar variant. No model configuration or credentials are modified.

For only the disabled-state PNG/RGBA pairs, run `node tools/render-brand.cjs --tray-states-only` with sharp on NODE_PATH. This mode preserves every existing logo, ICO and enabled tray resource.

For fold animation intermediates, run `node tools/render-brand.cjs --tray-animation-only`. It writes only `src-tauri/icons/tray-fold-light-32x32.rgba` and `tray-fold-dark-32x32.rgba`: 23 transparent 32×32 RGBA poses per theme. The first 19 ordinary poses progress from 1/20 to 19/20 of the selected rotation and scale and retain their previous bytes. Four slightly tighter poses (21/20 through 24/20) are appended for the double-click charge. Poses 0 and 20 still use the exact existing enabled/collapsed resources. The strips total 188,416 bytes (184 KiB), an increase of 32 KiB; they need no runtime image decoder or new dependency. Full regeneration also includes these strips.

`app/tray.rs` starts a visual preview on the first release, using a 260ms quadratic ease-out for ordinary folding/unfolding. Double-click takes over with a short charge and release back to the saved shape. Actual selection preference changes still await single-click confirmation. Unchanged icon poses and tooltip text are not resubmitted; animation tasks stop on completion, failure, supersession or exit. Frames use the existing window thread; there is no extra window, permanent animation thread or user setting. Lifecycle and acceptance boundaries are documented in `docs/tray-mode-feedback.md`.

Startup animation is integrated in `src/startup/` and `src-tauri/src/app/startup.rs`. `ribbon-mask.svg` and `lettering.svg` provide separate external SVG masks for the continuous sweep. Behavior and verification are documented in `docs/startup-animation.md`.
