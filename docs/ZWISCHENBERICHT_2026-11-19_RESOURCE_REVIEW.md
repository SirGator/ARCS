# ARCS – Gerätereservierungen im Desktop klären, 19. November 2026

Unter **Steuerung → Reservierung für Ziel …** findet der Betreiber beendete
Aufträge, deren Sitzung noch Geräteressourcen hält. **Ressourcen klären**
zeigt die gehaltenen Versionen und die vorhandenen Referenzen auf Ausführung
und Wirkungsbewertung. ARCS findet diese Sitzungen aus den aktuellen
Reservierungen im Artifact-Netz, unabhängig von der Liste der letzten Goals.
[Architekturspezifikation, PAR-04/PAR-10 und LNG-08/LNG-09](ARCHITECTURE.md)

Der Betreiber wählt vorhandene Artefaktversionen aus und kann ihre Inhalte
im Dialog prüfen. Nach Prüfung des Gerätezustands bestätigt er die Übergabe
mit einer Begründung. Wenn sich die Belegauswahl ändert, lädt der Dialog eine
neue Vorschau auf Anforderung. Die Freigabe vergleicht die exakten gehaltenen
Revisionen und übernimmt Begründung und Belegreferenzen in die neuen
Core-Versionen. [Bedienung im Kontrollpanel](../apps/control-panel/README.md)

Der Monitor liefert eine Vorschau mit unveränderten Artefakten und lehnt
unvollständige oder veraltete Reservierungssätze sowie fehlende Belege ab.
Der GTK-Test deckt Abbruch, Belegvorschau, Übergabe und Neustart mit dem
lokalen Lüftersimulator ab. Im Test war genau eine simulierte Geräteaktion
gespeichert. [Protokoll und Belege](../apps/adapter-host/examples/exclusive-goals.md)

Die vollständige Wiederholung auf einer privaten GTK-Anzeige bestand
46 Tests. Dazu kamen 33 erfolgreiche Rust-Tests für Betreiberbefehle,
Aufgabensteuerung sowie exklusive Reservierungen in Host und Core.
Workspace-Build und Formatprüfung liefen erfolgreich. Clippy meldete die
zwei vorhandenen Warnungen im Core-Reasoning-Service. Lokale Logs liegen
unter `var/verification-resource-review/`.

Controller-Fencing für reale Geräte und die Langzeitabnahme stehen aus.
