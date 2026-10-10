# Material icon assets

- MaterialIcons-Regular.ttf and its codepoints: [Google Material Icons](https://github.com/google/material-design-icons/tree/master/font).
- MaterialSymbols-Metrics.ttf: subset of [Google Material Symbols Outlined](https://github.com/google/material-design-icons/tree/master/variablefont). Static axes: FILL=0, GRAD=0, opsz=24, wght=400. Generated with FontTools 4.66.1.
- Both icon assets use the bundled Apache-2.0 LICENSE.txt.

| Metric | Material symbol | Codepoint |
|---|---|---|
| Size | format_size | E245 |
| Baseline | swap_vert | E8D5 |
| Tracking | format_letter_spacing | F773 |
| Vertical scale | height | EA16 |
| Horizontal scale | arrow_range | F69B |

The metrics subset contains only the five glyphs above and is embedded in a separate egui font family. Japanese font fallback cannot mask the private-use glyphs. The original variable font and FontTools are only needed to regenerate the subset, not to build or run the plugin.
