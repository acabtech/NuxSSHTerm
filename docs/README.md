# NuxSSHTerm Documentation

This directory contains technical and product-facing documentation for NuxSSHTerm.

## Documents

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — system architecture, backend/frontend responsibilities, data/storage model, integrations, and implementation notes.
- [`UI-SPEC.md`](UI-SPEC.md) — current UI overview plus historical WinSSHTerm-parity notes for context.
- [`screenshots/`](screenshots/) — reference screenshots (some may reflect earlier UI versions).

## Related project docs

- [`../README.md`](../README.md) — project overview, setup, build, and usage.
- [`../plans/DESIGN.md`](../plans/DESIGN.md) — UI design direction/tokens used for the modernized shell.
- [`../plans/modernization-plan.md`](../plans/modernization-plan.md) — modernization goals and IA notes.

## Maintenance guidance

- Prefer documenting **current behavior** and clearly labeling historical or planned content.
- When updating features (vault, import/export, key manager, SFTP, terminal/layout), keep `README.md`, `docs/ARCHITECTURE.md`, and `docs/UI-SPEC.md` in sync.
- Use repository-relative links and keep file paths/command names accurate to the implementation.
