# Dictionaries

These data files are licensed **separately** from the `egui_spellcheck` crate
(which is MPL-2.0). See `THIRD_PARTY_NOTICES.md` at the repository root and the
`*_license.*` files in this directory for the exact terms.

## US English dictionary

- Files: `en_US.dic`, `en_US.aff`
- Word list based on WordNet 2.1 / SCOWL.
- Upstream: <https://github.com/JetBrains/hunspell-dictionaries>
- Licenses: `en_US_license.txt`, `en_US_WordNet_license.txt`

## US Medical dictionary

- Files: `medical.dic`, `medical.aff`
- A **generated** supplemental clinical word list (see
  `src/medical_dictionary_generator/`), derived from the **UMLS Metathesaurus**.
- The **public build shipped here** uses only freely redistributable sources:
  **RxNorm**, **ICD-10-CM**, and **HPO** (Human Phenotype Ontology).
- Restrictively licensed sources such as **SNOMED CT** and **MedDRA** are
  **not** included in this file; they can be enabled only via the generator's
  `--include-licensed` switch for personal / authorized use, and such a build
  must not be redistributed.
- License & attributions: `medical_license.md`.
