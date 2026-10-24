#!/usr/bin/env python3
"""ARCS Control – natives Desktop-Fenster, keine Browser-Oberfläche."""
import argparse
from pathlib import Path
import shutil
import sys


def desktop_launcher(root: Path) -> Path:
    def quote(value: str) -> str:
        return '"' + value.replace("%", "%%").replace("\\", "\\\\").replace('"', '\\"').replace("`", "\\`").replace("$", "\\$") + '"'
    launcher = root / "ARCS-Control.desktop"
    icon = root / "apps/control-panel/arcs_panel/assets/arcs.svg"
    launcher.write_text("[Desktop Entry]\nType=Application\nName=ARCS Control\n"
        "Comment=ARCS starten, Adapter und Aufgaben überwachen\n"
        f"Exec={quote(str(root / 'arcs-control'))}\nIcon={icon}\n"
        "Terminal=false\nCategories=Development;Utility;\nStartupNotify=true\nStartupWMClass=arcs-control\n", encoding="utf-8")
    launcher.chmod(0o755)
    return launcher


def run() -> int:
    parser = argparse.ArgumentParser(description="ARCS Control: natives Kontrollpanel")
    parser.add_argument("--config", type=Path, help="Host-Konfiguration beim Öffnen auswählen")
    parser.add_argument("--runtime", type=Path, help="Optionale Aufgaben-Runtime")
    launchers = parser.add_mutually_exclusive_group()
    launchers.add_argument("--create-launcher", action="store_true", help="Desktop-Starter im Projektordner erzeugen")
    launchers.add_argument("--install-launcher", action="store_true", help="ARCS Control zum persönlichen Anwendungsmenü hinzufügen")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    if args.create_launcher or args.install_launcher:
        source = desktop_launcher(root)
        if args.install_launcher:
            target = Path.home() / ".local/share/applications/org.arcs.Control.desktop"
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
            target.chmod(0o644)
            print(target)
        else:
            print(source)
        return 0
    if args.runtime and not args.config:
        parser.error("--runtime benötigt --config")
    try:
        from arcs_panel.app import launch
    except (ImportError, ValueError) as error:
        print(f"Die Desktop-Bibliothek GTK 3 fehlt: {error}\nBenötigt werden Python 3, PyGObject und GTK 3.", file=sys.stderr)
        return 1
    return launch(args.config, args.runtime)


if __name__ == "__main__":
    raise SystemExit(run())
