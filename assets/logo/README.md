# Sakatsu logo

The standalone **屠** mark uses the exact filled outline in the app's rail,
`ui/src/lib/Mark.svelte`. It is vector artwork, with no font dependency.

- **sakatsu-mark.svg**: transparent vector master, editable in Inkscape.
- **sakatsu-mark-1024.png**: transparent 1024 × 1024 export.
- **sakatsu-avatar.svg**: square avatar with the rail's charcoal background.
- **sakatsu-avatar-1024.png**: recommended upload for sakatsu-dev and sakatsu-reader.
- **sakatsu-avatar-512.png**: smaller square export.

The mark keeps the rail's pink (`#e2687a`) and its original proportions and open
counters. The avatar uses the rail surface (`#121016`), with padding for square
and circular presentation. It adds no border, text, or seal plate.

Regenerate with Inkscape installed:

```powershell
python scripts/export-logo.py
```

The exporter reads the current rail outline and default colors, creates the SVGs
and PNGs with Inkscape, and refuses output if the path geometry changes during
export. Add `--app-icons` to refresh the UI logo and installed application icons
from the same avatar master used by the sakatsu-dev organization. The rail keeps
its unframed outline. Artwork is distributed under the project's GPL-3.0-only licence.
