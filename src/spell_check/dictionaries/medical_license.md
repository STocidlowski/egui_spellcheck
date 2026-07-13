# Medical dictionary — source data license & attributions

_Last updated: 2026-07-13._

The `medical.dic` / `medical.aff` files are a **generated word list**, not a
copy of any source database. They are produced by
`src/medical_dictionary_generator/` from the **UMLS Metathesaurus** (and,
optionally, a standalone RxNorm release). Only individual dictionary words
survive generation — no codes, identifiers, hierarchies, relationships or
definitions from the source vocabularies are reproduced.

The generator code is licensed under MPL-2.0, but the **data** below is governed
by the following third-party licenses. See the repository-root
`THIRD_PARTY_NOTICES.md` for the full statement.

## Sources in the public build (freely redistributable)

### RxNorm (SAB = RXNORM)
Produced by the U.S. National Library of Medicine (NLM). No additional UMLS
redistribution restrictions (UMLS category 0); also available as a fully open
standalone release. Required attribution:

> This product uses publicly available data courtesy of the U.S. National
> Library of Medicine (NLM), National Institutes of Health, Department of Health
> and Human Services; NLM is not responsible for the product and does not
> endorse or recommend this or any other product.

### ICD-10-CM (SAB = ICD10CM)
Produced by the U.S. CDC/NCHS. A work of the U.S. Government; public domain
within the United States. No attribution is required.

### HPO — Human Phenotype Ontology (SAB = HPO)
© The Human Phenotype Ontology Consortium. Provided under the HPO license
(<https://hpo.jax.org/app/license>), permitting commercial and non-commercial
use/redistribution **with attribution**. Required attribution:

> This product includes content from the Human Phenotype Ontology
> (<https://hpo.jax.org>). See Köhler S, et al. "The Human Phenotype Ontology in
> 2021." Nucleic Acids Research.

## Access to the source data — UMLS Metathesaurus

Downloading/using the UMLS Metathesaurus source files (`MRCONSO.RRF`, etc.)
requires accepting the UMLS Metathesaurus License Agreement with the NLM
(<https://www.nlm.nih.gov/research/umls/>). Anyone regenerating `medical.dic`
must hold a valid UMLS license (RxNorm may instead be obtained from its
standalone open release).

## Restrictive sources — excluded from the public build

The generator can add these via `--include-licensed`, but the resulting
dictionary MUST NOT be redistributed and is **not** the file shipped here:

- **SNOMED CT US Edition** (SAB = SNOMEDCT_US) — requires a SNOMED CT Affiliate
  License (<https://www.snomed.org/get-snomed>). © SNOMED International.
- **MedDRA** (SAB = MDR) — requires a MedDRA subscription
  (<https://www.meddra.org>); redistribution prohibited.
