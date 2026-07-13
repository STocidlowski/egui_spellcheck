"""
Medical Spellcheck Generator - "medical_spellcheck"

Project goals: parse UMLS / RxNorm and develop two files: "medical.aff" and "medical.dic" files for spellchecking for Hunspell and MySpell dictionaries.
These are used in LibreOffice, OpenOffice

Existing Repository examples: https://github.com/wooorm/dictionaries

Dictionaries are in the dictionary folder at the project root

.dic (Dictionary): A plain text file where each line contains a word, optionally followed by flags separated by a slash (e.g., word/flags). The first line simply indicates the approximate total number of words in the file.
.aff (Affix): A companion file defining morphological prefix and suffix rules. When checking spelling, Hunspell uses these rules to identify root words and validate any alterations.
"""

import argparse
import logging
import os
import re
from dataclasses import dataclass
from itertools import chain, islice
from typing import Iterable, Iterator, Optional

from enums import (
    UMLSLanguageOfTerm,
    UMLSStringType,
    UMLSSuppresssFlag,
    UMLSTermAbbreviation,
    UMLSTermStatus,
)
from models import MetathesaurusConcept

logger = logging.getLogger("medical_spellcheck")

UMLS_LOCATION = r"C:\Users\Shawn\PycharmProjects\downloads\umls_meta_extracted\umls-2025AA-metathesaurus-full\2025AA\META"
MRCONSO_FILE = r"MRCONSO.RRF"

# Semantic-type file (CUI -> semantic type / TUI). Used to drop whole *kinds* of
# concept - specifically the taxonomy / organism hierarchy (species of bacteria,
# fungi, viruses, plants, animals, ...), which contributes tens of thousands of
# obscure Latin species names that no clinician dictates. Common pathogens
# (Staphylococcus, Streptococcus, Candida, ...) are retained because they also
# appear in non-organism *disease* concepts that are not filtered out here.
MRSTY_FILE = r"MRSTY.RRF"

# UMLS Semantic Network TUIs that make up the "Organisms" hierarchy. Concepts
# carrying any of these are taxonomy entries (genus/species/strain names) and
# are excluded wholesale. See https://lhncbc.nlm.nih.gov/semanticnetwork/ .
ORGANISM_TUIS = {
    "T001",  # Organism
    "T002",  # Plant
    "T004",  # Fungus
    "T005",  # Virus
    "T007",  # Bacterium
    "T008",  # Animal
    "T010",  # Vertebrate
    "T011",  # Amphibian
    "T012",  # Bird
    "T013",  # Fish
    "T014",  # Reptile
    "T015",  # Mammal
    "T016",  # Human
    "T194",  # Archaeon
    "T204",  # Eukaryote
}

# Output directory (project-local "dictionaries" folder).
PROJECT_ROOT = os.path.dirname(os.path.abspath(__file__))
OUTPUT_DIR = os.path.join(PROJECT_ROOT, "dictionaries")

# Reference dictionary used to drop words that the base English dictionary
# already covers. The medical list is meant to *supplement* en_US, so any word
# already present there is redundant noise in medical.dic.
EN_US_DIC = os.path.join(OUTPUT_DIR, "en_US.dic")

# Companion affix file for the reference dictionary. When available, its PFX/SFX
# rules are expanded against en_US.dic so that *derived* forms (e.g.
# "unwillingness" from "willing/UPY", "unwinding" from "wind/UASG") are also
# treated as already covered and removed from medical.dic.
EN_US_AFF = os.path.join(OUTPUT_DIR, "en_US.aff")

# Pluralisation suffix flag (must match the SFX rule defined in medical.aff).
PLURAL_FLAG = "S"

# Genitive ("'s") suffix flag (must match the SFX rule defined in medical.aff).
# Mirrors the en_US.aff flag "M" (SFX M 0 's .) so the two dictionaries form the
# possessive consistently.
GENITIVE_FLAG = "M"

# Affix file content. Keeps a single, simple pluralisation suffix so that the
# generated word list stays small while still accepting regular plural forms.
AFF_CONTENT = """SET UTF-8
TRY esianrtolcdugmphbyfvkwz

# Split words at hyphens and check each part independently (this is Hunspell's
# default behaviour, made explicit here). It lets repetitive hyphenated
# compounds such as "cytokine-induced" be accepted from their parts
# ("cytokine" + "induced") without storing every combination, mirroring how the
# base en_US dictionary handles hyphenation.
BREAK 3
BREAK -
BREAK ^-
BREAK -$

# Minimal affix rules for the supplemental medical word list. A simple
# pluralisation suffix (flag S) is provided so e.g. "statins"/"troponins" are
# accepted, plus a genitive suffix (flag M) for the possessive "'s" form (e.g.
# "Tourette's", "Towne's"); the full English affix machinery lives in en_US.aff.
# Keep this file intentionally small. The S and M rule lines are intentionally
# identical to en_US.aff so the two dictionaries inflect consistently.
SFX S Y 4
SFX S   y     ies        [^aeiou]y
SFX S   0     s          [aeiou]y
SFX S   0     es         [sxzh]
SFX S   0     s          [^sxzhy]

SFX M Y 1
SFX M   0     's          .
"""

# A "word" we are willing to add to the dictionary: a run of letters that may
# contain internal hyphens or apostrophes (e.g. "well-being", "Crohn's").
WORD_RE = re.compile(r"^[A-Za-z]+(?:[-'][A-Za-z]+)*$")

# Amino acid / nucleotide sequence tokens: two or more single letters joined by
# hyphens, e.g. "A-T", "A-T-G-G", "a-t-a-t-t-t-g-g-c-g-a-t-g-g-c-t-t-c". These
# are biological sequence notations, not real words, so they are dropped.
SEQUENCE_RE = re.compile(r"^[A-Za-z](?:-[A-Za-z])+$")

# Three-letter amino acid codes (standard 20 + selenocysteine/pyrrolysine,
# ambiguity codes, and common non-standard residues). Used to detect peptide
# sequence notations such as "Val-Leu-Asn-Tyr-Tyr-Val-Trp" or
# "Val-Lys-PABC-DOX" which are chemical sequences, not real words.
AMINO_ACID_CODES = {
    "ala", "arg", "asn", "asp", "cys", "gln", "glu", "gly", "his", "ile",
    "leu", "lys", "met", "phe", "pro", "ser", "thr", "trp", "tyr", "val",
    "sec", "pyl", "asx", "glx", "xaa", "xle",
    "orn", "hyp", "nle", "nva", "abu", "aib", "cit", "sar", "dab", "dap",
    "hci", "hse", "hyl", "pen", "cha", "nal", "gla", "aad",
}


def is_amino_acid_sequence(token: str) -> bool:
    """Return ``True`` for hyphen-joined peptide sequences (e.g. ``Val-Leu``).

    A token is treated as an amino acid / peptide sequence when it is made of
    two or more hyphen-separated components and at least two of them are known
    three-letter amino acid codes that also form the majority of components.
    This catches pure sequences (``Val-Leu-Asn-Tyr``) and capped/modified ones
    (``Val-Leu-Gly-OH``, ``Val-Lys-PABC-DOX``, ``Val-MeArg-Gly-Asp``) while
    leaving real hyphenated words (``well-being``, ``I-Macroaggregated``,
    ``Pro-American``) untouched.
    """
    parts = token.split("-")
    if len(parts) < 2:
        return False
    aa = sum(1 for part in parts if part.lower() in AMINO_ACID_CODES)
    return aa >= 2 and aa * 2 >= len(parts)


# Spelled-out amino acid *acyl* residue names (the "-yl" forms used to name
# peptides, e.g. "glutamyl", "leucyl", "phenylalanyl"). Unlike the three-letter
# codes above, these are unambiguous chemistry terms that never appear as
# components of ordinary hyphenated English words, so a single one is enough to
# flag a hyphen-joined token as a systematic peptide/chemical name.
AMINO_ACID_ACYL = {
    "glycyl", "alanyl", "valyl", "leucyl", "isoleucyl", "prolyl",
    "phenylalanyl", "tryptophyl", "tryptophanyl", "methionyl", "seryl",
    "threonyl", "cysteinyl", "cystyl", "tyrosyl", "asparaginyl", "glutaminyl",
    "aspartyl", "aspartoyl", "glutamyl", "glutaryl", "lysyl", "arginyl",
    "histidyl",
    # Modified / non-standard residues commonly seen in UMLS peptide strings.
    "ornithyl", "citrullyl", "hydroxyprolyl", "hydroxylysyl", "selenocysteinyl",
    "pyroglutamyl", "sarcosyl", "norleucyl", "norvalyl", "homoseryl",
    "homocysteinyl", "alloisoleucyl", "pipecolyl", "betaalanyl",
}

# Full amino acid names. On their own these are legitimate dictionary words
# (e.g. "phenylalanine", "glycine"), so they only count toward peptide
# detection when they appear as a *component* of a hyphen-joined token.
AMINO_ACID_NAMES = {
    "glycine", "alanine", "valine", "leucine", "isoleucine", "proline",
    "phenylalanine", "tryptophan", "methionine", "serine", "threonine",
    "cysteine", "cystine", "tyrosine", "asparagine", "glutamine",
    "aspartate", "aspartic", "glutamate", "glutamic", "lysine", "arginine",
    "histidine", "ornithine", "citrulline", "selenocysteine",
    "hydroxyproline", "homoserine", "homocysteine", "norleucine", "norvaline",
}


def is_peptide_name(token: str) -> bool:
    """Return ``True`` for spelled-out peptide / acyl chemical names.

    UMLS contains systematic peptide names written with full amino-acid acyl
    residue names ("-yl" forms) and/or full amino-acid names joined by hyphens,
    e.g. `glutamyl-leucyl-aspartyl-...`, `glutamyl-phenylalanine`,
    `GLUTAMYL-L-TYROSYL-L-PROLYL` or short fragments like `glutamyl-lysyl`.
    These are chemical notations, not real words.

    A hyphen-joined token is treated as such a name when either:

    * at least one part is a known amino-acid acyl residue name (these are
      unambiguous chemistry terms), or
    * at least two parts are amino-acid acyl names or full amino-acid
      names.

    Single (non-hyphenated) tokens such as `phenylalanine` or `glutamyl`
    are left untouched, as are ordinary hyphenated words (`well-being`,
    `Pro-American`).
    """
    parts = token.split("-")
    if len(parts) < 2:
        return False
    acyl = 0
    aa = 0
    for part in parts:
        lower = part.lower()
        if lower in AMINO_ACID_ACYL:
            acyl += 1
            aa += 1
        elif lower in AMINO_ACID_NAMES:
            aa += 1
    return acyl >= 1 or aa >= 2


# Term types (TTY) that do not contribute to clinical terms a clinician would use
# in dictation. Dropping these at the record level removes the *source* of the
# noise (systematic chemical names, machine codes/abbreviations, obsolete
# content, machine-formatted drug strings) instead of pattern-matching each
# generated token. The goal is an aggressively pruned, clinically focused list,
# so we keep human-readable preferred/synonym/ingredient/finding/drug names and
# drop everything that is a code, an abbreviation, a machine format, obsolete,
# or a systematic chemical string.
EXCLUDED_TTY = {
    # --- Systematic chemical / CAS names & MeSH Supplementary Concept Records.
    # Overwhelmingly IUPAC/CAS strings and chemical substance names, not words.
    UMLSTermAbbreviation.CCN,   # Chemical code name
    UMLSTermAbbreviation.CHN,   # Chemical structure name
    UMLSTermAbbreviation.CSN,   # Chemical Structure Name
    UMLSTermAbbreviation.N1,    # Chemical Abstracts Service Type 1 name
    UMLSTermAbbreviation.NM,    # Name of Supplementary Concept
    UMLSTermAbbreviation.CE,    # Entry term for a Supplementary Concept
    UMLSTermAbbreviation.PCE,   # Preferred entry term for Supplementary Concept
    # --- Abbreviations, acronyms and short forms. A clinician dictates the full
    # word, not these contracted/initialism forms (and they are rarely real
    # words on their own).
    UMLSTermAbbreviation.AA,    # Attribute type abbreviation
    UMLSTermAbbreviation.AB,    # Abbreviation in any source vocabulary
    UMLSTermAbbreviation.ACR,   # Acronym
    UMLSTermAbbreviation.MTH_ACR,  # MTH acronym
    UMLSTermAbbreviation.AM,    # Short form of modifier
    UMLSTermAbbreviation.DS,    # Short form of descriptor
    UMLSTermAbbreviation.ES,    # Short form of entry term
    UMLSTermAbbreviation.NS,    # Short form of non-preferred term
    UMLSTermAbbreviation.OSN,   # Official short name
    UMLSTermAbbreviation.PS,    # Short forms that needed full specification
    UMLSTermAbbreviation.QAB,   # Qualifier abbreviation
    UMLSTermAbbreviation.RAB,   # Root abbreviation
    UMLSTermAbbreviation.SS,    # Synonymous "short" forms
    UMLSTermAbbreviation.SSN,   # Source short name (Knowledge Source Server)
    UMLSTermAbbreviation.VAB,   # Versioned abbreviation
    UMLSTermAbbreviation.OA,    # Obsolete abbreviation
    UMLSTermAbbreviation.OAM,   # Obsolete Modifier Abbreviation
    # --- Codes / identifiers that are not natural-language words.
    UMLSTermAbbreviation.CA2,   # ISO country code (alpha-2)
    UMLSTermAbbreviation.CA3,   # ISO country code (alpha-3)
    UMLSTermAbbreviation.CCS,   # FIPS 10-4 country code
    UMLSTermAbbreviation.CSY,   # Code system
    UMLSTermAbbreviation.HTN,   # HL7 Table Name
    # --- Machine-formatted drug name strings (concatenated/delimited/abbreviated
    # formats with no real spacing); the human-readable drug names come from
    # other TTYs (IN, PIN, BN, SCD, SBD, ...).
    UMLSTermAbbreviation.CDA,   # Clinical drug name in abbreviated format
    UMLSTermAbbreviation.CDC,   # Clinical drug name in concatenated format
    UMLSTermAbbreviation.CDD,   # Clinical drug name in delimited format
    # --- Obsolete content (no longer current clinical usage). This complements
    # the SUPPRESS=O filter for sources that flag obsolescence only via TTY.
    UMLSTermAbbreviation.IS,    # Obsolete Synonym
    UMLSTermAbbreviation.LO,    # Obsolete official fully specified name
    UMLSTermAbbreviation.OAF,   # Obsolete active fully specified name
    UMLSTermAbbreviation.OAP,   # Obsolete active preferred term
    UMLSTermAbbreviation.OAS,   # Obsolete active synonym
    UMLSTermAbbreviation.ODN,   # Obsolete Display Name
    UMLSTermAbbreviation.OET,   # Obsolete entry term
    UMLSTermAbbreviation.OF,    # Obsolete fully specified name
    UMLSTermAbbreviation.OLC,   # Obsolete Long common name
    UMLSTermAbbreviation.OLG,   # Obsolete LOINC group name
    UMLSTermAbbreviation.OL,    # Non-current Lower Level Term
    UMLSTermAbbreviation.OM,    # Obsolete modifiers in HCPCS
    UMLSTermAbbreviation.ONP,   # Obsolete non-preferred for language term
    UMLSTermAbbreviation.OOSN,  # Obsolete official short name
    UMLSTermAbbreviation.OPN,   # Obsolete preferred term, natural language form
    UMLSTermAbbreviation.OP,    # Obsolete preferred name
    UMLSTermAbbreviation.MTH_IS,   # MTH-supplied form of obsolete synonym
    UMLSTermAbbreviation.MTH_OAF,  # MTH obsolete active fully specified name
    UMLSTermAbbreviation.MTH_OAP,  # MTH obsolete active preferred term
    UMLSTermAbbreviation.MTH_OAS,  # MTH obsolete active synonym
    UMLSTermAbbreviation.MTH_OET,  # MTH obsolete entry term
    UMLSTermAbbreviation.MTH_OF,   # MTH obsolete fully specified name
    UMLSTermAbbreviation.MTH_OL,   # MTH Non-current Lower Level Term
    UMLSTermAbbreviation.MTH_OPN,  # MTH obsolete preferred (natural language)
    UMLSTermAbbreviation.MTH_OP,   # MTH obsolete preferred term
}


# Source vocabularies (SAB) to KEEP. This is the single biggest quality/size
# lever. UMLS aggregates ~100 sources; most of the bloat is low-yield for
# clinical dictation. In particular the NCBI Taxonomy (SAB "NCBI") alone
# contributes ~377k unique words - overwhelmingly Latin species/strain names of
# bacteria, fungi, viruses and other organisms that a clinician never dictates.
# Restricting to a curated, high-yield clinical set removes that noise at the
# record level (the source), which is far more reliable than trying to pattern
# match obscure terms one by one.
#
# LICENSING NOTE (read before changing these sets):
# The source vocabularies are split into two groups by *redistribution* rights,
# because the generated word list is a derivative of the source content:
#
#   FREE_SAB     - sources that may be freely redistributed (the derived word
#                  list can ship publicly, e.g. bundled in this repository under
#                  the project's MPL-2.0 code license). These are UMLS "category
#                  0" style vocabularies with no additional downstream fees or
#                  affiliate agreements:
#                    RXNORM  - normalised clinical drug/ingredient/brand names
#                              (NLM; also available as a standalone, fully free
#                              RxNorm release, see --rxnorm-location).
#                    ICD10CM - billable diagnosis names (US CDC/NCHS, public
#                              domain in the United States).
#                    HPO     - Human Phenotype Ontology clinical phenotype
#                              descriptions (permissive HPO license, attribution
#                              required).
#
#   LICENSED_SAB - clinically valuable but RESTRICTIVELY licensed sources whose
#                  content (and therefore any derived word list) may NOT be
#                  redistributed without a separate agreement. These are opt-in
#                  only, via --include-licensed, and the resulting dictionary is
#                  for personal / authorized use and MUST NOT be published:
#                    SNOMEDCT_US - SNOMED CT US Edition. Requires a SNOMED CT
#                                  Affiliate License (free for use within UMLS
#                                  in Member territories, but redistribution is
#                                  governed by SNOMED International).
#                    MDR         - MedDRA. Requires a MedDRA subscription from
#                                  the MSSO; redistribution is prohibited.
#
# Deliberately excluded examples (available via --include-sab): NCBI
# (taxonomy/species), NCI & MSH (large, research-/chemical-heavy thesauri),
# LNC/LOINC (verbose lab strings), GO/HGNC (gene/molecular biology),
# OMIM/ORPHANET (rare-disease catalogues), plus dozens of small single-source
# synonym sets. Note that many of these carry their own restrictive licenses.
FREE_SAB = {
    "RXNORM",
    "ICD10CM",
    "HPO",
}

LICENSED_SAB = {
    "SNOMEDCT_US",
    "MDR",
}

# Default allow-list: only the freely redistributable sources, so the bundled
# output can be published. Enable the licensed sources with --include-licensed.
INCLUDED_SAB = set(FREE_SAB)


# A curated allow-list of high-yield clinical terms that MUST be present in the
# final dictionary. As the filters get more aggressive there is a risk of
# accidentally pruning a genuinely useful word; this set is a safety net. After
# all filtering, any of these terms not already present (and not already covered
# by the base en_US dictionary) is re-added, and a warning is logged. Keep it
# focused on words a clinician actually dictates across the common specialties.
MUST_HAVE_TERMS = {
    # Cardiology
    "myocardial", "infarction", "angina", "arrhythmia", "tachycardia",
    "bradycardia", "fibrillation", "ischemia", "ischemic", "atherosclerosis",
    "cardiomyopathy", "pericarditis", "endocarditis", "hypertension",
    "hypotension", "hyperlipidemia", "dyslipidemia",
    # Pulmonary
    "pneumonia", "asthma", "emphysema", "bronchitis", "bronchiectasis",
    "dyspnea", "tachypnea", "hypoxia", "hypoxemia", "pneumothorax",
    "atelectasis", "hemoptysis", "wheezing",
    # GI / hepatology
    "appendicitis", "cholecystitis", "cholelithiasis", "pancreatitis",
    "diverticulitis", "gastritis", "gastroenteritis", "hepatitis",
    "cirrhosis", "colitis", "dysphagia", "hematemesis", "melena",
    "esophagogastroduodenoscopy", "colonoscopy", "cholecystectomy",
    "appendectomy", "colectomy",
    # Neurology
    "seizure", "epilepsy", "migraine", "meningitis", "encephalitis",
    "neuropathy", "paresthesia", "aphasia", "ataxia", "syncope",
    "hemiparesis", "hemiplegia", "dementia",
    # Renal / endocrine / metabolic
    "nephropathy", "nephritis", "pyelonephritis", "proteinuria",
    "hematuria", "hyperkalemia", "hypokalemia", "hyponatremia",
    "hypernatremia", "hyperglycemia", "hypoglycemia", "diabetes",
    "hypothyroidism", "hyperthyroidism", "ketoacidosis",
    # Heme / onc / infectious
    "anemia", "leukocytosis", "leukopenia", "thrombocytopenia",
    "neutropenia", "coagulopathy", "lymphoma", "leukemia", "carcinoma",
    "sarcoma", "melanoma", "metastasis", "metastatic", "sepsis",
    "bacteremia", "cellulitis", "abscess",
    # Common medications
    "acetaminophen", "ibuprofen", "aspirin", "amoxicillin", "azithromycin",
    "ciprofloxacin", "metformin", "insulin", "lisinopril", "atorvastatin",
    "metoprolol", "amlodipine", "omeprazole", "furosemide", "warfarin",
    "heparin", "prednisone", "albuterol", "gabapentin", "levothyroxine",
    # General exam / status
    "afebrile", "febrile", "edema", "erythema", "jaundice", "pallor",
    "cyanosis", "malaise", "lethargy", "tachycardic", "hypertensive",
}


# MRCONSO.RRF is a pipe ("|") delimited file. Column order (0-based):
# 0 CUI, 1 LAT, 2 TS, 3 LUI, 4 STT, 5 SUI, 6 ISPREF, 7 AUI, 8 SAUI, 9 SCUI,
# 10 SDUI, 11 SAB, 12 TTY, 13 CODE, 14 STR, 15 SRL, 16 SUPPRESS, 17 CVF
_NUM_FIELDS = 18


def _to_optional(value: str) -> Optional[str]:
    """Empty MRCONSO fields are optional values."""
    return value if value != "" else None


def parse_line(line: str) -> Optional[MetathesaurusConcept]:
    """Parse a single MRCONSO.RRF line into a :class:`MetathesaurusConcept`.

    Returns `None` for blank lines or malformed records.
    """
    line = line.rstrip("\n").rstrip("\r")
    if not line:
        return None
    fields = line.split("|")
    # Each well-formed record ends with a trailing "|", producing one extra,
    # empty element after split().
    if len(fields) < _NUM_FIELDS:
        return None
    try:
        return MetathesaurusConcept(
            CUI=fields[0],
            LAT=UMLSLanguageOfTerm(fields[1]),
            TS=UMLSTermStatus(fields[2]),
            LUI=fields[3],
            STT=UMLSStringType(fields[4]) if fields[4] else None,
            SUI=fields[5],
            ISPREF=fields[6] == "Y",
            AUI=fields[7],
            SAUI=_to_optional(fields[8]),
            SCUI=_to_optional(fields[9]),
            SDUI=_to_optional(fields[10]),
            SAB=fields[11],
            TTY=UMLSTermAbbreviation(fields[12]),
            CODE=fields[13],
            STR=fields[14],
            SRL=int(fields[15]) if fields[15] else 0,
            SUPPRESS=UMLSSuppresssFlag(fields[16]),
            CVF=int(fields[17]) if fields[17] else None,
        )
    except ValueError:
        # Unknown enum value / non-numeric field - skip the record rather than
        # aborting the whole parse over a single bad row.
        return None


def iter_concepts(path: str, limit: Optional[int] = None) -> Iterator[MetathesaurusConcept]:
    """Stream :class:`MetathesaurusConcept` records from an MRCONSO.RRF file."""
    count = 0
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for raw in handle:
            concept = parse_line(raw)
            if concept is None:
                continue
            yield concept
            count += 1
            if limit is not None and count >= limit:
                break


def is_relevant(
    concept: MetathesaurusConcept,
    included_sab: Optional[set[str]] = None,
) -> bool:
    """Keep only English, non-suppressed concept names from high-yield sources.

    Filtering, in order:

    * language must be English and the record must not be suppressed;
    * the source vocabulary (``SAB``) must be in ``included_sab`` (defaults to
      :data:`INCLUDED_SAB`) - this is the biggest quality/size lever, removing
      whole low-yield sources such as the NCBI Taxonomy (bacteria/species
      names). Pass an empty set to accept *all* sources;
    * the term type (``TTY``) must not be in :data:`EXCLUDED_TTY` - systematic
      chemical/CAS names, MeSH Supplementary Concept Record names,
      abbreviations/acronyms/short forms, machine codes, machine-formatted drug
      strings and obsolete content, none of which are dictatable clinical terms.
    """
    if included_sab is None:
        included_sab = INCLUDED_SAB
    if concept.LAT != UMLSLanguageOfTerm.ENG:
        return False
    if concept.SUPPRESS in (UMLSSuppresssFlag.O, UMLSSuppresssFlag.Y, UMLSSuppresssFlag.E):
        return False
    if included_sab and concept.SAB not in included_sab:
        return False
    if concept.TTY in EXCLUDED_TTY:
        return False
    return True


def extract_words(text: str) -> Iterator[str]:
    """Split a concept string into individual dictionary words.

    Splits on whitespace and on punctuation that is not part of a word (commas,
    parentheses, slashes, etc.), then yields tokens that look like real words.
    """
    # Replace anything that is not a letter, hyphen or apostrophe with a space,
    # then split. This drops digits, parentheses, commas, dosage units, etc.
    for token in re.split(r"[^A-Za-z'\-]+", text):
        token = token.strip("-'")
        if len(token) < 2:
            continue
        if SEQUENCE_RE.match(token):
            # Amino acid / nucleotide sequence (e.g. "A-T-G-G"); not a word.
            continue
        if is_amino_acid_sequence(token):
            # Peptide sequence (e.g. "Val-Leu-Asn-Tyr"); not a word.
            continue
        if is_peptide_name(token):
            # Spelled-out peptide / acyl name (e.g. "glutamyl-leucyl-..."); not
            # a word.
            continue
        if WORD_RE.match(token):
            yield token


def load_excluded_cuis(
    path: str, tuis: Iterable[str] = ORGANISM_TUIS
) -> set[str]:
    """Load the CUIs whose semantic type (TUI) is in ``tuis`` from MRSTY.RRF.

    MRSTY.RRF is pipe-delimited as ``CUI|TUI|STN|STY|ATUI|CVF|``. The returned
    set is used to drop whole concepts (e.g. the taxonomy / organism hierarchy)
    regardless of source or term type. Returns an empty set if the file is
    missing (the filter then becomes a no-op).
    """
    tuis = set(tuis)
    cuis: set[str] = set()
    if not path or not os.path.exists(path):
        logger.warning("Semantic-type file %s not found; skipping organism filter", path)
        return cuis
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for raw in handle:
            parts = raw.split("|", 2)
            if len(parts) >= 2 and parts[1] in tuis:
                cuis.add(parts[0])
    return cuis


def build_word_set(
    concepts: Iterable[MetathesaurusConcept],
    included_sab: Optional[set[str]] = None,
    exclude_cuis: Optional[set[str]] = None,
) -> set[str]:
    """Build the de-duplicated set of dictionary words from concept records.

    ``included_sab`` is forwarded to :func:`is_relevant` (the source-vocabulary
    allow-list); ``None`` uses the default :data:`INCLUDED_SAB`. ``exclude_cuis``
    is an optional set of concept identifiers to skip entirely (e.g. the
    taxonomy / organism CUIs from :func:`load_excluded_cuis`).
    """
    words: set[str] = set()
    for concept in concepts:
        if exclude_cuis and concept.CUI in exclude_cuis:
            continue
        if not is_relevant(concept, included_sab=included_sab):
            continue
        for word in extract_words(concept.STR):
            words.add(word)
    return words


def load_reference_words(path: str) -> set[str]:
    """Load the (lower-cased) head words of a Hunspell/MySpell `.dic` file.

    Each `.dic` line is `word` optionally followed by `/flags` and/or
    space-separated morphological data. We only need the head word. The first
    line (an approximate word count) and blank lines are ignored. Words are
    lower-cased, so the comparison against the medical list is case-insensitive
    (e.g. `Albumin` is treated as already covered by `albumin`).
    """
    reference: set[str] = set()
    if not path or not os.path.exists(path):
        logger.warning("Reference dictionary %s not found; skipping de-duplication", path)
        return reference
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        first = True
        for raw in handle:
            line = raw.rstrip("\n").rstrip("\r").strip()
            if first:
                first = False
                # Skip the leading count line (it is just a number).
                if line.isdigit():
                    continue
            if not line:
                continue
            # Drop morphological data (after the first whitespace) and the
            # affix flags (after the first "/").
            head = line.split(None, 1)[0]
            head = head.split("/", 1)[0]
            head = head.strip("-'")
            if head:
                reference.add(head.lower())
    return reference


def remove_reference_words(words: Iterable[str], reference: set[str]) -> set[str]:
    """Drop words already present in ``reference`` (compared case-insensitively)."""
    if not reference:
        return set(words)
    return {word for word in words if word.lower() not in reference}


def collapse_hyphenated_words(
    words: Iterable[str], reference: Optional[set[str]] = None
) -> set[str]:
    """Drop hyphenated words whose every component is independently coverable.

    Hunspell's default ``BREAK`` table (also set explicitly in ``medical.aff``)
    splits an unknown word at hyphens and re-checks each part, so a hyphenated
    compound like ``cytokine-induced`` is accepted as long as both ``cytokine``
    and ``induced`` are accepted on their own. Storing every such combination
    (``Cytokine-associated``, ``Cytokine-based``, ``Cytokine-induced`` ...) is
    therefore redundant and only bloats the list.

    A hyphenated word is dropped when *every* one of its hyphen-separated
    components is independently coverable, i.e. each component (compared
    case-insensitively) is either:

    * present in ``reference`` (the base en_US dictionary, affix-expanded), or
    * present as a standalone, non-hyphenated word in ``words``.

    Hyphenated words with a component that is not otherwise covered (e.g.
    ``I-Macroaggregated``) are kept, since Hunspell could not validate them
    from their parts alone.
    """
    words = set(words)
    reference = reference or set()
    # Words that can stand on their own: the base dictionary plus every
    # non-hyphenated medical word.
    coverable = reference | {w.lower() for w in words if "-" not in w}
    result: set[str] = set()
    for word in words:
        if "-" in word:
            parts = word.split("-")
            if all(part and part.lower() in coverable for part in parts):
                continue
        result.add(word)
    return result


def collapse_case_variants(words: Iterable[str]) -> set[str]:
    """Collapse case-only duplicates (``Cytokine``/``CYTOKINE``/``cytokine``).

    Hunspell accepts the capitalised and all-caps forms of a *lower-case* root
    automatically, so storing every case variant is redundant. For each group of
    words that share a lower-cased form:

    * if the lower-case surface form is present, keep **only** it (the Title-case
      and ALL-CAPS variants are then accepted for free), otherwise
    * the word never occurs in lower case (a genuinely capitalised brand name,
      eponym or acronym such as ``Tylenol`` or ``MRSA``); keep a single best
      capitalised form - the one with the *fewest* upper-case letters (prefer
      ``Tylenol`` over ``TYLENOL``), since a Title-case entry also accepts its
      all-caps form whereas an all-caps entry does not accept the Title-case
      one.

    This is the largest easy intra-list win and respects the project's intent of
    keeping legitimately capitalised names (those simply have no lower-case form
    to collapse onto).
    """
    groups: dict[str, list[str]] = {}
    for word in words:
        groups.setdefault(word.lower(), []).append(word)
    result: set[str] = set()
    for lower, variants in groups.items():
        if lower in variants:
            result.add(lower)
            continue
        # No lower-case form: keep the variant with the fewest capital letters
        # (ties broken alphabetically for deterministic output).
        best = min(
            variants,
            key=lambda w: (sum(1 for ch in w if ch.isupper()), w),
        )
        result.add(best)
    return result


def ensure_must_have_terms(
    words: set[str],
    must_have: Iterable[str] = MUST_HAVE_TERMS,
    reference: Optional[set[str]] = None,
) -> set[str]:
    """Guarantee high-yield clinical terms survive aggressive filtering.

    Any term in ``must_have`` that is neither already present (case-insensitive)
    nor covered by ``reference`` (the base en_US dictionary, lower-cased) is
    re-added to ``words``. This is a safety net so that tightening the source /
    term-type filters can never silently drop an essential clinical word. The
    number of re-added and en_US-covered terms is logged.
    """
    reference = reference or set()
    present = {w.lower() for w in words}
    readded: list[str] = []
    covered: list[str] = []
    for term in must_have:
        low = term.lower()
        if low in present:
            continue
        if low in reference:
            covered.append(term)
            continue
        words.add(term)
        present.add(low)
        readded.append(term)
    if readded:
        logger.warning(
            "Re-added %d must-have term(s) missing after filtering: %s",
            len(readded),
            ", ".join(sorted(readded)),
        )
    if covered:
        logger.info(
            "%d must-have term(s) already covered by the base dictionary",
            len(covered),
        )
    return words


@dataclass
class AffixRule:
    """A single Hunspell PFX/SFX rule line.

    `kind` is `"PFX"` or `"SFX"`; `cross` records whether the class
    permits a prefix/suffix combination; `strip` is the characters removed from
    the stem (`""` for "0"); `add` is the affix appended/prepended; and
    `condition` is the compiled regex used to test applicability.
    """

    kind: str
    flag: str
    cross: bool
    strip: str
    add: str
    condition: re.Pattern


def _parse_affix_lines(lines: Iterable[str]) -> dict[str, list[AffixRule]]:
    """Parse PFX/SFX rules from already-read ``.aff`` lines.

    Shared by :func:`parse_affix_rules` (file on disk) and
    :func:`medical_affix_rules` (the in-memory :data:`AFF_CONTENT`). Returns a
    mapping ``flag -> [AffixRule, ...]``.
    """
    lines = list(lines)
    rules: dict[str, list[AffixRule]] = {}
    for raw in lines:
        line = raw.rstrip("\n").rstrip("\r").strip()
        if not line or (line[:3] not in ("PFX", "SFX")):
            continue
        parts = line.split()
        # Header line: "PFX flag cross count" (4 fields). Rule lines have a
        # strip/add/condition payload (>= 5 fields after the flag).
        if len(parts) < 5:
            continue
        kind, flag, strip, add = parts[0], parts[1], parts[2], parts[3]
        condition = parts[4]
        # Strip continuation flags from the affix ("able/MS" -> "able").
        add = add.split("/", 1)[0]
        strip = "" if strip == "0" else strip
        add = "" if add == "0" else add
        # The cross-product flag lives on the class header, not the rule
        # line, so it is filled in by the second pass below.
        try:
            regex = re.compile(("" if condition == "." else condition) + "$") if kind == "SFX" \
                else re.compile("^" + ("" if condition == "." else condition))
        except re.error:
            continue
        rules.setdefault(flag, []).append(
            AffixRule(kind=kind, flag=flag, cross=False, strip=strip, add=add, condition=regex)
        )
    # Second pass: read cross-product flags from the class headers and apply
    # them to every rule of that flag.
    for raw in lines:
        parts = raw.split()
        if len(parts) == 4 and parts[0] in ("PFX", "SFX") and parts[3].isdigit():
            flag, cross = parts[1], parts[2] == "Y"
            for rule in rules.get(flag, []):
                rule.cross = cross
    return rules


def parse_affix_rules(path: str) -> dict[str, list[AffixRule]]:
    """Parse the PFX/SFX rules of a Hunspell ``.aff`` file.

    Returns a mapping ``flag -> [AffixRule, ...]``. Only the affix-creation
    options (``PFX``/``SFX``) are needed for de-duplication; compounding,
    replacement, and other options are ignored.
    """
    if not path or not os.path.exists(path):
        return {}
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        return _parse_affix_lines(handle)


def medical_affix_rules() -> dict[str, list[AffixRule]]:
    """Parse the affix rules of the generated :data:`AFF_CONTENT` (medical.aff).

    Used by :func:`collapse_plurals` so the de-duplication uses exactly the same
    pluralisation rules that ship in ``medical.aff``.
    """
    return _parse_affix_lines(AFF_CONTENT.splitlines())


def _apply_suffix(word: str, rule: AffixRule) -> Optional[str]:
    if not rule.condition.search(word):
        return None
    if rule.strip:
        if not word.endswith(rule.strip):
            return None
        stem = word[: -len(rule.strip)]
    else:
        stem = word
    return stem + rule.add


def _apply_prefix(word: str, rule: AffixRule) -> Optional[str]:
    if not rule.condition.match(word):
        return None
    if rule.strip:
        if not word.startswith(rule.strip):
            return None
        stem = word[len(rule.strip):]
    else:
        stem = word
    return rule.add + stem


def expand_word(word: str, flags: str, rules: dict[str, list[AffixRule]]) -> set[str]:
    """Expand a single dictionary entry into all forms its affix flags allow.

    Generates the bare word, every prefix/suffix form, and prefix+suffix
    cross-products (when both classes permit cross-product). Twofold affix
    stripping is not modelled, which matches the simple en_US ruleset.
    """
    forms: set[str] = {word}
    prefix_rules = [r for f in flags for r in rules.get(f, []) if r.kind == "PFX"]
    suffix_rules = [r for f in flags for r in rules.get(f, []) if r.kind == "SFX"]

    suffixed: list[str] = []
    for rule in suffix_rules:
        new = _apply_suffix(word, rule)
        if new is not None:
            forms.add(new)
            suffixed.append(new)

    for rule in prefix_rules:
        new = _apply_prefix(word, rule)
        if new is None:
            continue
        forms.add(new)
        if rule.cross:
            # Combine this prefix with each cross-product suffix form.
            for suffix_rule in suffix_rules:
                if not suffix_rule.cross:
                    continue
                combined = _apply_prefix(_apply_suffix(word, suffix_rule) or word, rule)
                if combined is not None:
                    forms.add(combined)
    return forms


def load_reference_words_expanded(dic_path: str, aff_path: str) -> set[str]:
    """Load reference words and expand them with the companion `.aff` rules.

    Returns the lower-cased set of every form accepted by `dic_path` +
    `aff_path` (bare roots plus all affixed/derived forms). Falls back to the
    plain head-word loader when no affix rules are available.
    """
    rules = parse_affix_rules(aff_path)
    if not rules:
        return load_reference_words(dic_path)

    reference: set[str] = set()
    if not dic_path or not os.path.exists(dic_path):
        logger.warning("Reference dictionary %s not found; skipping de-duplication", dic_path)
        return reference
    with open(dic_path, "r", encoding="utf-8", errors="replace") as handle:
        first = True
        for raw in handle:
            line = raw.rstrip("\n").rstrip("\r").strip()
            if first:
                first = False
                if line.isdigit():
                    continue
            if not line:
                continue
            head = line.split(None, 1)[0]
            word, _, flags = head.partition("/")
            word = word.strip("-'")
            if not word:
                continue
            for form in expand_word(word, flags, rules):
                form = form.strip("-'")
                if form:
                    reference.add(form.lower())
    return reference


def _collapse_by_suffix(
    words: Iterable[str],
    flag: str,
    rules: Optional[dict[str, list[AffixRule]]] = None,
) -> tuple[set[str], dict[str, str]]:
    """Collapse derived suffix forms onto their base word for one SFX ``flag``.

    Shared by :func:`collapse_plurals` (flag ``S``) and :func:`collapse_genitives`
    (flag ``M``). For every base word, the SFX rules of ``flag`` are applied; if a
    generated form is also stored as its own head word (case-insensitively), the
    derived entry is dropped and the base word is tagged with ``flag`` so Hunspell
    re-expands it. The base word's original casing is preserved.

    Returns ``(remaining_words, flags)`` where ``flags`` maps a kept base word to
    the affix flag string it should be written with.
    """
    if rules is None:
        rules = medical_affix_rules()
    suffix_rules = [r for r in rules.get(flag, []) if r.kind == "SFX"]
    words = set(words)
    if not suffix_rules:
        return words, {}

    # Lower-cased lookup so a generated form can find its stored entry
    # regardless of casing. First-seen wins for deterministic behaviour.
    lower_map: dict[str, str] = {}
    for word in words:
        lower_map.setdefault(word.lower(), word)

    flags: dict[str, str] = {}
    removed: set[str] = set()
    # Process base words in a stable order so output is deterministic.
    for word in sorted(words, key=lambda w: (w.lower(), w)):
        if word in removed:
            continue
        for rule in suffix_rules:
            derived = _apply_suffix(word, rule)
            if not derived or derived.lower() == word.lower():
                continue
            target = lower_map.get(derived.lower())
            if target is None or target == word or target in removed:
                continue
            removed.add(target)
            flags[word] = flag
            break

    result = {word for word in words if word not in removed}
    return result, flags


def collapse_plurals(
    words: Iterable[str],
    rules: Optional[dict[str, list[AffixRule]]] = None,
) -> tuple[set[str], dict[str, str]]:
    """Collapse regular plural duplicates using the ``medical.aff`` SFX S rule.

    ``medical.aff`` ships an ``SFX S`` pluralisation rule but, until now, the
    generator never applied it: regular singular/plural pairs such as
    ``millilitre``/``millilitres`` were both stored as separate head words. This
    function removes the plural form whenever its singular is also present and
    tags the singular with the ``/S`` affix flag, so Hunspell still accepts the
    plural while the ``.dic`` shrinks (mirroring how the base ``en_US`` list
    stores ``root/S`` rather than the explicit plural).

    Matching is case-insensitive: the plural generated from a stored singular is
    looked up against the lower-cased word set, so e.g. ``millimole`` collapses
    ``Millimoles`` too (Hunspell accepts the capitalised form of a lower-case
    root). The singular's original casing is preserved.

    Returns ``(remaining_words, flags)`` where ``flags`` maps a kept singular to
    the affix flag string (``"S"``) it should be written with.
    """
    return _collapse_by_suffix(words, PLURAL_FLAG, rules)


def collapse_genitives(
    words: Iterable[str],
    rules: Optional[dict[str, list[AffixRule]]] = None,
) -> tuple[set[str], dict[str, str]]:
    """Collapse genitive (``'s``) duplicates using the ``medical.aff`` SFX M rule.

    UMLS supplies possessive eponyms both as a base form and as an explicit
    genitive (e.g. ``Towne`` + ``Towne's``, ``Tourette`` + ``Tourette's``). This
    drops the ``'s`` entry whenever its base word is also present and tags the
    base with the ``/M`` affix flag, so Hunspell still accepts the possessive
    while the ``.dic`` shrinks (mirroring how ``en_US`` stores ``root/M`` rather
    than the explicit ``root's``).

    Matching is case-insensitive and the base word's original casing is kept.

    Returns ``(remaining_words, flags)`` where ``flags`` maps a kept base word to
    the affix flag string (``"M"``) it should be written with.
    """
    return _collapse_by_suffix(words, GENITIVE_FLAG, rules)


def write_dictionary(
    words: Iterable[str],
    output_dir: str = OUTPUT_DIR,
    flags: Optional[dict[str, str]] = None,
) -> tuple[str, str]:
    """Write the `medical.dic` and `medical.aff` files.

    ``flags`` optionally maps a word to the Hunspell affix flag string it should
    carry (e.g. ``{"millilitre": "S"}`` -> ``millilitre/S``), which lets a single
    head word also accept its affixed (e.g. plural) forms.

    Returns the paths of the written `(dic, aff)` files.
    """
    os.makedirs(output_dir, exist_ok=True)
    flags = flags or {}
    # Case-insensitive, then case-sensitive sort for stable, readable output.
    sorted_words = sorted(set(words), key=lambda w: (w.lower(), w))

    dic_path = os.path.join(output_dir, "medical.dic")
    aff_path = os.path.join(output_dir, "medical.aff")

    with open(dic_path, "w", encoding="utf-8", newline="\n") as dic:
        dic.write(f"{len(sorted_words)}\n")
        for word in sorted_words:
            flag = flags.get(word)
            dic.write(f"{word}/{flag}\n" if flag else f"{word}\n")

    with open(aff_path, "w", encoding="utf-8", newline="\n") as aff:
        aff.write(AFF_CONTENT)

    return dic_path, aff_path


def generate(
    umls_location: str = UMLS_LOCATION,
    mrconso_file: str = MRCONSO_FILE,
    output_dir: str = OUTPUT_DIR,
    limit: Optional[int] = None,
    exclude_dic: Optional[str] = EN_US_DIC,
    exclude_aff: Optional[str] = EN_US_AFF,
    collapse_hyphens: bool = True,
    collapse_case: bool = True,
    dedup_plurals: bool = True,
    dedup_genitives: bool = True,
    ensure_must_have: bool = True,
    included_sab: Optional[set[str]] = None,
    include_licensed: bool = False,
    mrsty_file: Optional[str] = MRSTY_FILE,
    extra_sources: Optional[Iterable[str]] = None,
) -> tuple[str, str]:
    """End-to-end generation of the medical Hunspell/MySpell dictionary.

    ``included_sab`` selects the source vocabularies to keep; ``None`` uses the
    freely redistributable default (:data:`FREE_SAB`). ``include_licensed`` adds
    the restrictively licensed sources (:data:`LICENSED_SAB`, e.g. SNOMED CT and
    MedDRA); the resulting word list is a derivative of licensed content and
    MUST NOT be redistributed. ``extra_sources`` is an optional list of extra
    ``*CONSO.RRF`` file paths to parse in addition to the UMLS ``MRCONSO.RRF``
    (e.g. a standalone RxNorm ``RXNCONSO.RRF``); every source shares the same
    18-column layout.
    """
    sources = [os.path.join(umls_location, mrconso_file)]
    if extra_sources:
        sources.extend(extra_sources)
    for source in sources:
        logger.info("Parsing %s", source)
    if included_sab is None:
        included_sab = set(FREE_SAB)
    if include_licensed and included_sab:
        # Union in the restrictive sources. If included_sab is empty ("all"),
        # every source is already accepted so there is nothing to add.
        included_sab = included_sab | LICENSED_SAB
        logger.warning(
            "Including LICENSED sources (%s); the output is a derivative of "
            "restrictively licensed content and MUST NOT be redistributed.",
            ", ".join(sorted(LICENSED_SAB)),
        )
    logger.info(
        "Including %s source(s): %s",
        len(included_sab) if included_sab else "all",
        ", ".join(sorted(included_sab)) if included_sab else "(all)",
    )
    exclude_cuis: set[str] = set()
    if mrsty_file:
        # Drop taxonomy / organism concepts (species of bacteria, fungi, etc.).
        exclude_cuis = load_excluded_cuis(os.path.join(umls_location, mrsty_file))
        logger.info("Excluding %d organism/taxonomy concepts (via MRSTY)", len(exclude_cuis))
    concepts = chain.from_iterable(iter_concepts(src) for src in sources)
    if limit is not None:
        concepts = islice(concepts, limit)
    words = build_word_set(
        concepts,
        included_sab=included_sab,
        exclude_cuis=exclude_cuis,
    )
    logger.info("Extracted %d unique words", len(words))
    reference: set[str] = set()
    if exclude_dic:
        # Expand the reference dictionary with its affix rules so derived forms
        # (e.g. "unwillingness", "unwinding") are also treated as covered.
        if exclude_aff:
            reference = load_reference_words_expanded(exclude_dic, exclude_aff)
        else:
            reference = load_reference_words(exclude_dic)
        if reference:
            before = len(words)
            words = remove_reference_words(words, reference)
            logger.info(
                "Removed %d words already present in %s; %d remain",
                before - len(words),
                exclude_dic,
                len(words),
            )
    if collapse_case:
        # Collapse case-only duplicates ("Cytokine"/"CYTOKINE"/"cytokine").
        # Hunspell accepts capitalised forms of a lower-case root, so only the
        # lower-case form needs storing; genuinely capitalised-only names
        # (brands/eponyms) are preserved.
        before = len(words)
        words = collapse_case_variants(words)
        logger.info(
            "Collapsed %d case-variant duplicates; %d remain",
            before - len(words),
            len(words),
        )
    if collapse_hyphens:
        # Drop repetitive hyphenated compounds (e.g. "cytokine-induced") whose
        # parts are each independently coverable, relying on the affix file's
        # BREAK rules to still accept them in prose.
        before = len(words)
        words = collapse_hyphenated_words(words, reference)
        logger.info(
            "Collapsed %d redundant hyphenated words; %d remain",
            before - len(words),
            len(words),
        )
    if ensure_must_have:
        # Safety net: re-add any essential clinical term that aggressive
        # filtering may have removed (unless already covered by en_US).
        before = len(words)
        words = ensure_must_have_terms(words, reference=reference)
        if len(words) != before:
            logger.info("Final word count after must-have re-add: %d", len(words))
    head_flags: dict[str, str] = {}
    if dedup_plurals:
        # Apply the medical.aff "SFX S" rule: drop a stored plural when its
        # singular is also present and tag the singular with "/S" so Hunspell
        # still accepts the plural (e.g. "millilitre/S" replaces the pair
        # "millilitre" + "millilitres").
        before = len(words)
        words, plural_flags = collapse_plurals(words)
        for word, flag in plural_flags.items():
            head_flags[word] = head_flags.get(word, "") + flag
        logger.info(
            "Collapsed %d plural duplicates onto singular/S forms; %d remain",
            before - len(words),
            len(words),
        )
    if dedup_genitives:
        # Apply the medical.aff "SFX M" rule: drop a stored genitive when its
        # base word is also present and tag the base with "/M" so Hunspell still
        # accepts the possessive (e.g. "Towne/M" replaces "Towne" + "Towne's").
        before = len(words)
        words, genitive_flags = collapse_genitives(words)
        for word, flag in genitive_flags.items():
            head_flags[word] = head_flags.get(word, "") + flag
        logger.info(
            "Collapsed %d genitive ('s) duplicates onto base/M forms; %d remain",
            before - len(words),
            len(words),
        )
    dic_path, aff_path = write_dictionary(words, output_dir=output_dir, flags=head_flags)
    logger.info("Wrote %s and %s", dic_path, aff_path)
    return dic_path, aff_path


def _parse_included_sab(value: Optional[str]) -> Optional[set[str]]:
    """Turn the --include-sab CLI value into a set for :func:`generate`.

    ``None`` -> use the default :data:`INCLUDED_SAB`; ``"all"`` (any case) ->
    empty set (accept every source); otherwise a comma-separated list of source
    abbreviations.
    """
    if value is None:
        return None
    value = value.strip()
    if not value or value.lower() == "all":
        return set()
    return {part.strip() for part in value.split(",") if part.strip()}


def _parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate medical.aff / medical.dic from UMLS MRCONSO.RRF")
    parser.add_argument("--umls-location", default=UMLS_LOCATION, help="Directory containing MRCONSO.RRF")
    parser.add_argument("--mrconso-file", default=MRCONSO_FILE, help="MRCONSO file name")
    parser.add_argument("--output-dir", default=OUTPUT_DIR, help="Directory to write the dictionary files into")
    parser.add_argument("--limit", type=int, default=None, help="Only read the first N records (useful for testing)")
    parser.add_argument(
        "--exclude-dic",
        default=EN_US_DIC,
        help="Hunspell .dic whose words are removed from the output (default: en_US.dic). Pass an empty string to disable.",
    )
    parser.add_argument(
        "--exclude-aff",
        default=EN_US_AFF,
        help="Companion .aff for --exclude-dic; its affix rules are expanded so derived forms are also removed (default: en_US.aff). Pass an empty string to match only bare head words.",
    )
    parser.add_argument(
        "--keep-hyphenated",
        dest="collapse_hyphens",
        action="store_false",
        help="Keep redundant hyphenated compounds whose parts are each independently coverable (by default they are collapsed and accepted via the affix BREAK rules).",
    )
    parser.add_argument(
        "--keep-case-variants",
        dest="collapse_case",
        action="store_false",
        help="Keep case-only duplicates (e.g. Cytokine/CYTOKINE/cytokine) instead of collapsing them onto the lower-case root.",
    )
    parser.add_argument(
        "--keep-plurals",
        dest="dedup_plurals",
        action="store_false",
        help="Keep explicit plural entries instead of collapsing them onto a singular '/S' head word (e.g. keep both 'millilitre' and 'millilitres' rather than 'millilitre/S').",
    )
    parser.add_argument(
        "--keep-genitives",
        dest="dedup_genitives",
        action="store_false",
        help="Keep explicit genitive entries instead of collapsing them onto a base '/M' head word (e.g. keep both 'Towne' and \"Towne's\" rather than 'Towne/M').",
    )
    parser.add_argument(
        "--no-must-have",
        dest="ensure_must_have",
        action="store_false",
        help="Do not re-add the curated must-have clinical terms after filtering.",
    )
    parser.add_argument(
        "--mrsty-file",
        default=MRSTY_FILE,
        help=(
            "Semantic-type file used to drop taxonomy/organism concepts "
            "(species of bacteria, fungi, ...). Default: MRSTY.RRF. Pass an "
            "empty string to disable the organism filter."
        ),
    )
    parser.add_argument(
        "--include-sab",
        default=None,
        help=(
            "Comma-separated source vocabularies (SAB) to keep (default: the "
            "freely redistributable FREE_SAB set: "
            + ", ".join(sorted(FREE_SAB))
            + "). Pass 'all' to accept every source. NOTE: many other sources "
            "carry restrictive licenses; only publish output built from freely "
            "redistributable sources."
        ),
    )
    parser.add_argument(
        "--include-licensed",
        dest="include_licensed",
        action="store_true",
        help=(
            "Also include the restrictively licensed sources ("
            + ", ".join(sorted(LICENSED_SAB))
            + "). Requires the appropriate SNOMED CT Affiliate / MedDRA "
            "licenses. The resulting dictionary is a derivative of licensed "
            "content and MUST NOT be redistributed (personal / authorized use "
            "only)."
        ),
    )
    parser.add_argument(
        "--rxnorm-location",
        default=None,
        help=(
            "Optional directory containing a standalone RxNorm release "
            "(RXNCONSO.RRF). When given, its concepts are parsed in addition "
            "to the UMLS MRCONSO.RRF. RxNorm is fully free to redistribute."
        ),
    )
    parser.add_argument(
        "--rxnorm-file",
        default="RXNCONSO.RRF",
        help="RxNorm concept file name inside --rxnorm-location (default: RXNCONSO.RRF).",
    )
    return parser.parse_args(argv)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    args = _parse_args()
    extra_sources: list[str] = []
    if args.rxnorm_location:
        extra_sources.append(os.path.join(args.rxnorm_location, args.rxnorm_file))
    generate(
        umls_location=args.umls_location,
        mrconso_file=args.mrconso_file,
        output_dir=args.output_dir,
        limit=args.limit,
        exclude_dic=args.exclude_dic or None,
        exclude_aff=args.exclude_aff or None,
        collapse_hyphens=args.collapse_hyphens,
        collapse_case=args.collapse_case,
        dedup_plurals=args.dedup_plurals,
        dedup_genitives=args.dedup_genitives,
        ensure_must_have=args.ensure_must_have,
        included_sab=_parse_included_sab(args.include_sab),
        include_licensed=args.include_licensed,
        mrsty_file=args.mrsty_file or None,
        extra_sources=extra_sources or None,
    )
