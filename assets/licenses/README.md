# Supplemental upstream legal texts

These files supplement crate archives that omit their repository's top-level legal texts. `sources.json` maps exact Cargo.lock package names/versions to immutable upstream Git commits and raw source URLs. Texts are copied unchanged. Packaging uses only entries matching its host-filtered resolved dependency inventory.

Some old upstream archives/repositories (notably `dispatch` 0.2.0 and `hexf-parse` 0.2.1) declare a license in Cargo metadata but provide no standalone license document. Those are recorded with their original authors, license declaration and source metadata in the package; no copyright statements or license grants have been invented. This inventory is prototype attribution, not an assertion of a complete commercial distribution audit.

Opterm's own MIT and Apache-2.0 license texts are at the repository root. Font notices come from the fonts' original sources and the locked `epaint_default_fonts` archive.
