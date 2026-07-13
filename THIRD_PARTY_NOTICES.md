# Third-Party Notices

`egui_spellcheck`'s own source code is licensed under the Mozilla Public
License 2.0 (see `LICENSE`). The bundled **dictionary data files** under
`src/spell_check/dictionaries/` are **not** covered by that license; they are
derived from third-party sources and are governed by the separate licenses and
notices documented below.

---

## en_US Hunspell dictionary

- Files: `en_US.aff`, `en_US.dic`
- Purpose: base English word list, used both as a bundled dictionary and as the
  de-duplication reference for the medical word list.
- Upstream: Hunspell `en_US` dictionary (SCOWL / WordNet-derived word list, as
  redistributed by the JetBrains `hunspell-dictionaries` collection —
  <https://github.com/JetBrains/hunspell-dictionaries>).
- Local changes: none (redistributed verbatim).
- Licenses and notices: the word list is based on WordNet (Princeton University)
  and SCOWL; see the included license files for the exact applicable terms.
- Included license files: `src/spell_check/dictionaries/en_US_license.txt`,
  `src/spell_check/dictionaries/en_US_WordNet_license.txt`.

---

## Medical terminology dictionary

- Files: `medical.aff`, `medical.dic`
- Generator: `src/medical_dictionary_generator/` (`main.py`), MPL-2.0.
- Local transformation: the word list is **generated** from the source
  vocabularies below. The generator streams the UMLS `MRCONSO.RRF` (and,
  optionally, a standalone RxNorm `RXNCONSO.RRF`), keeps only English,
  non-suppressed concepts from an allow-list of source vocabularies, strips
  codes/abbreviations/systematic chemical and biological-sequence strings,
  removes words already covered by `en_US`, and collapses case / hyphen /
  plural / genitive variants. See `src/medical_dictionary_generator/README.md`
  for the full pipeline. Only individual **words** survive; no codes,
  hierarchies, relationships or definitions are reproduced.

### Sources included in the bundled (public) `medical.dic`

The bundled dictionary is built **only** from freely redistributable sources
(the generator's default `FREE_SAB` set). Each retains its own copyright and
required attribution:

- **RxNorm** (`SAB = RXNORM`) — U.S. National Library of Medicine (NLM).
  RxNorm carries no additional UMLS redistribution restrictions (UMLS
  category 0) and is also published as a fully open standalone release.
  Required attribution:

  > This product uses publicly available data courtesy of the U.S. National
  > Library of Medicine (NLM), National Institutes of Health, Department of
  > Health and Human Services; NLM is not responsible for the product and does
  > not endorse or recommend this or any other product.

- **ICD-10-CM** (`SAB = ICD10CM`) — U.S. Centers for Disease Control and
  Prevention / National Center for Health Statistics (CDC/NCHS). A work of the
  U.S. Government; in the public domain within the United States. No attribution
  required.

- **HPO — Human Phenotype Ontology** (`SAB = HPO`) — © The Human Phenotype
  Ontology Consortium, provided under the Human Phenotype Ontology license
  (<https://hpo.jax.org/app/license>), which permits commercial and
  non-commercial use and redistribution **with attribution**. Required
  attribution:

  > This product includes content from the Human Phenotype Ontology
  > (<https://hpo.jax.org>). See Köhler S, et al. "The Human Phenotype Ontology
  > in 2021." Nucleic Acids Research.

### UMLS Metathesaurus (access to the source data)

The generator reads its input from the **UMLS Metathesaurus**. Downloading and
using the UMLS Metathesaurus source files requires accepting the *UMLS
Metathesaurus License Agreement* with the NLM
(<https://www.nlm.nih.gov/research/umls/>). That agreement governs access to
the raw source data; it does not add redistribution restrictions to a derived
word list built solely from category-0 sources (RxNorm, ICD-10-CM) and HPO.
Anyone regenerating `medical.dic` must hold a valid UMLS license (RxNorm may
instead be obtained from its standalone open release).

### Restrictively licensed sources — NOT included in the public build

The generator can additionally include the following sources via
`--include-licensed`, but they are **deliberately excluded from the bundled,
publicly released `medical.dic`** because their content (and any derived word
list) may not be redistributed without a separate agreement. A dictionary built
with `--include-licensed` is for personal / authorized use only and **must not
be published**:

- **SNOMED CT US Edition** (`SAB = SNOMEDCT_US`) — © SNOMED International (the
  U.S. Edition is maintained by the NLM). Use and redistribution require a
  SNOMED CT Affiliate License (<https://www.snomed.org/get-snomed>). Use within
  UMLS is free in SNOMED International Member territories (including the U.S.),
  but redistribution of SNOMED-derived content is governed by that license.

- **MedDRA** (`SAB = MDR`) — Medical Dictionary for Regulatory Activities,
  © the MedDRA trademark holder, maintained by the MSSO. Use requires a MedDRA
  subscription (<https://www.meddra.org>); redistribution is prohibited.

Numerous other UMLS sources available via `--include-sab` (e.g. NCI Thesaurus,
MeSH, LOINC) carry their own restrictive terms; consult the UMLS *Source
Vocabulary Documentation* and each source's license before distributing any
dictionary built from them.
