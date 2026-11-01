"""Export the rail's exact vector mark and square avatars with Inkscape.

Run: python scripts/export-logo.py [--inkscape PATH]
The output is assets/logo/. No fonts, tracing or changes to the app artwork.
"""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parent.parent
SVG = "http://www.w3.org/2000/svg"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inkscape", type=Path)
    parser.add_argument("--app-icons", action="store_true",
                        help="also refresh the UI logo and generate application icons with Tauri")
    args = parser.parse_args()
    executable = args.inkscape or shutil.which("inkscape")
    if not executable and os.name == "nt":
        candidate = Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Inkscape/bin/inkscape.com"
        if candidate.is_file():
            executable = candidate
    if not executable:
        parser.error("Inkscape is required. Install it or pass --inkscape PATH.")

    component = (ROOT / "ui/src/lib/Mark.svelte").read_text(encoding="utf-8")
    paths = re.findall(r'<path\s+d="([^"]+)"', component)
    if len(paths) != 1 or 'fill-rule="evenodd"' not in component:
        raise SystemExit("The rail artwork changed; review the extraction before exporting.")
    css = (ROOT / "ui/src/theme.css").read_text(encoding="utf-8")
    ink = re.search(r"--red:\s*(#[0-9a-fA-F]{6});", css).group(1)
    background = re.search(r"--surface:\s*(#[0-9a-fA-F]{6});", css).group(1)
    path = f'<path fill="{ink}" fill-rule="evenodd" d="{paths[0]}"/>'

    def document(name, body):
        return (
            f'<svg xmlns="{SVG}" width="1000" height="1000" viewBox="0 0 1000 1000">'
            f'<title>{name}</title>{body}</svg>\n'
        )

    originals = {
        "sakatsu-mark": document("Sakatsu Reader mark", path),
        "sakatsu-avatar": document(
            "Sakatsu Reader avatar",
            f'<rect width="1000" height="1000" fill="{background}"/>'
            '<svg x="140" y="140" width="720" height="720" viewBox="0 0 1000 1000" '
            f'overflow="hidden">{path}</svg>',
        ),
    }
    out = ROOT / "assets/logo"
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="sakatsu-logo-") as directory:
        temporary = Path(directory)
        env = os.environ.copy()
        env["INKSCAPE_PROFILE_DIR"] = str(temporary / "profile")

        def export(source, destination, *options):
            command = [str(executable), str(source), "--export-area-page",
                       f"--export-filename={destination}", *options]
            result = subprocess.run(
                command, env=env, capture_output=True, text=True,
                encoding="utf-8", errors="replace",
                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
            )
            if result.returncode or not destination.is_file():
                raise SystemExit(f"Inkscape export failed: {result.stderr.strip()}")

        ready = []
        for name, contents in originals.items():
            source = temporary / f"{name}-source.svg"
            source.write_text(contents, encoding="utf-8")
            vector = temporary / f"{name}.svg"
            export(source, vector, "--export-plain-svg")
            tree = ET.parse(vector)
            exported = tree.findall(f".//{{{SVG}}}path")
            if [element.get("d") for element in exported] != paths:
                raise SystemExit("Inkscape changed the path geometry; outputs not published.")
            ready.append(vector)
            for size in ([1024] if name == "sakatsu-mark" else [512, 1024]):
                raster = temporary / f"{name}-{size}.png"
                export(vector, raster, f"--export-width={size}", f"--export-height={size}")
                ready.append(raster)

        # Generate the complete set before replacing any existing deliverable.
        for source in ready:
            destination = out / source.name
            replacement = destination.with_suffix(destination.suffix + ".partial")
            replacement.write_bytes(source.read_bytes())
            replacement.replace(destination)
    if args.app_icons:
        # Generate separately so an exporter failure cannot partially replace app icons.
        with tempfile.TemporaryDirectory(prefix="sakatsu-icons-") as directory:
            icon_dir = Path(directory) / "icons"
            subprocess.run(["cargo", "tauri", "icon", str(out / "sakatsu-avatar.svg"),
                            "--output", str(icon_dir)], cwd=ROOT, check=True)
            for source in icon_dir.rglob("*"):
                if source.is_file():
                    destination = ROOT / "src-tauri/icons" / source.relative_to(icon_dir)
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    replacement = destination.with_suffix(destination.suffix + ".partial")
                    replacement.write_bytes(source.read_bytes())
                    replacement.replace(destination)
        ui_logo = ROOT / "ui/src/assets/brand/sakatsu.svg"
        ui_logo.parent.mkdir(parents=True, exist_ok=True)
        ui_logo.write_bytes((out / "sakatsu-avatar.svg").read_bytes())
        (ROOT / "src-tauri/icons/source.png").write_bytes((out / "sakatsu-avatar-1024.png").read_bytes())
    print("Exported the exact rail outline using Inkscape" +
          (", including UI and application icons." if args.app_icons else "."))


if __name__ == "__main__":
    main()
