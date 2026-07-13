"""Unit tests for the medical spellcheck generator."""

import os
import tempfile

from enums import UMLSLanguageOfTerm, UMLSSuppresssFlag, UMLSTermAbbreviation
from main import (
    FREE_SAB,
    INCLUDED_SAB,
    LICENSED_SAB,
    MUST_HAVE_TERMS,
    ORGANISM_TUIS,
    _parse_included_sab,
    build_word_set,
    collapse_case_variants,
    collapse_genitives,
    collapse_hyphenated_words,
    collapse_plurals,
    ensure_must_have_terms,
    expand_word,
    extract_words,
    is_relevant,
    load_excluded_cuis,
    load_reference_words,
    load_reference_words_expanded,
    medical_affix_rules,
    parse_affix_rules,
    parse_line,
    remove_reference_words,
    write_dictionary,
)

AFF_SAMPLE = """SET UTF-8

PFX U Y 1
PFX U   0     un         .

PFX A Y 1
PFX A   0     re         .

SFX G Y 2
SFX G   e     ing        e
SFX G   0     ing        [^e]

SFX P Y 3
SFX P   y     iness      [^aeiou]y
SFX P   0     ness       [aeiou]y
SFX P   0     ness       [^y]

SFX S Y 4
SFX S   y     ies        [^aeiou]y
SFX S   0     s          [aeiou]y
SFX S   0     es         [sxzh]
SFX S   0     s          [^sxzhy]
"""

SAMPLE = (
    "C0000005|ENG|P|L0000005|PF|S0007492|Y|A26634265||M0019694|D012711|MSH|PEP|"
    "D012711|(131)I-Macroaggregated Albumin|0|N|256|"
)
FRENCH = (
    "C0000005|FRE|P|L6220710|PF|S7133957|Y|A13433185||M0019694|D012711|MSHFRE|PEP|"
    "D012711|Albumine|3|N||"
)
SUPPRESSED = (
    "C0000005|ENG|S|L0270109|PF|S0007491|Y|A26634266||M0019694|D012711|MSH|ET|"
    "D012711|Obsoleteword|0|O|256|"
)
# A MeSH Supplementary Concept Record name (TTY=NM): a systematic chemical name
# that should be dropped by the TTY filter even though it is English and not
# suppressed.
CHEMICAL_NM = (
    "C0000123|ENG|S|L0000123|PF|S0000123|Y|A00000123|||C012345|MSH|NM|"
    "C012345|2-(acetyloxy)benzoic acid compound|0|N|256|"
)
# An abbreviation (TTY=AB): a contracted form, not a word a clinician dictates.
ABBREVIATION_AB = (
    "C0000234|ENG|S|L0000234|PF|S0000234|Y|A00000234|||012345|SNOMEDCT_US|AB|"
    "012345|MI|0|N|256|"
)
# An obsolete entry term (TTY=OET): non-current content dropped by the filter.
OBSOLETE_OET = (
    "C0000345|ENG|S|L0000345|PF|S0000345|Y|A00000345|||012346|SNOMEDCT_US|OET|"
    "012346|Oldterm|0|N|256|"
)
# A clinical concept from an included source (SNOMEDCT_US) with a dictatable
# term type (PT): kept by the default SAB allow-list.
SNOMED_SAMPLE = (
    "C0004238|ENG|P|L0004238|PF|S0016288|Y|A02923541|||49436004|SNOMEDCT_US|PT|"
    "49436004|Atrial fibrillation|9|N|256|"
)
# An English, non-suppressed concept from a source that is NOT in the default
# allow-list (NCBI Taxonomy - a bacterium species name): dropped by the SAB
# filter even though language/suppress/TTY would otherwise keep it.
NCBI_SPECIES = (
    "C0314745|ENG|P|L0314745|PF|S0357757|Y|A00000999|||562|NCBI|SCN|"
    "562|Escherichia coli|0|N|256|"
)


def test_parse_line_basic():
    concept = parse_line(SAMPLE)
    assert concept is not None
    assert concept.CUI == "C0000005"
    assert concept.LAT == UMLSLanguageOfTerm.ENG
    assert concept.STR == "(131)I-Macroaggregated Albumin"
    assert concept.SUPPRESS == UMLSSuppresssFlag.N
    assert concept.ISPREF is True


def test_parse_line_blank_and_malformed():
    assert parse_line("") is None
    assert parse_line("not|enough|fields") is None


def test_is_relevant_language_and_suppress():
    # included_sab=set() accepts all sources, isolating the language/suppress
    # checks from the SAB allow-list (SAMPLE is from MSH, not in the default).
    assert is_relevant(parse_line(SAMPLE), included_sab=set()) is True
    assert is_relevant(parse_line(FRENCH), included_sab=set()) is False
    assert is_relevant(parse_line(SUPPRESSED), included_sab=set()) is False


def test_is_relevant_sab_filter():
    # By default only the freely redistributable sources (FREE_SAB) are kept.
    snomed = parse_line(SNOMED_SAMPLE)
    assert snomed is not None and snomed.SAB == "SNOMEDCT_US"
    # SNOMED CT is a restrictively licensed source, excluded from the free
    # default so the bundled output can be published.
    assert is_relevant(snomed) is False
    # ...but kept when the licensed sources are explicitly included.
    assert is_relevant(snomed, included_sab=FREE_SAB | LICENSED_SAB) is True
    # A non-allow-listed source (NCBI taxonomy) is dropped by default...
    species = parse_line(NCBI_SPECIES)
    assert species is not None and species.SAB == "NCBI"
    assert is_relevant(species) is False
    # ...but accepted when the SAB filter is disabled (empty set = all sources).
    assert is_relevant(species, included_sab=set()) is True
    # MSH is not in the default allow-list either.
    assert is_relevant(parse_line(SAMPLE)) is False


def test_is_relevant_drops_chemical_tty():
    # Chemical-name-heavy term types (here NM, a MeSH SCR name) are dropped even
    # when English and non-suppressed. included_sab=set() isolates the TTY check
    # (the fixture is from MSH, outside the default SAB allow-list).
    concept = parse_line(CHEMICAL_NM)
    assert concept is not None
    assert concept.TTY == UMLSTermAbbreviation.NM
    assert is_relevant(concept, included_sab=set()) is False


def test_is_relevant_drops_abbreviation_and_obsolete_tty():
    # Abbreviations/acronyms/short forms and obsolete content are dropped even
    # when English and non-suppressed.
    # included_sab=set() isolates the TTY check from the SAB allow-list (the
    # fixtures are SNOMEDCT_US, a licensed source outside the free default).
    ab = parse_line(ABBREVIATION_AB)
    assert ab is not None
    assert ab.TTY == UMLSTermAbbreviation.AB
    assert is_relevant(ab, included_sab=set()) is False

    oet = parse_line(OBSOLETE_OET)
    assert oet is not None
    assert oet.TTY == UMLSTermAbbreviation.OET
    assert is_relevant(oet, included_sab=set()) is False


def test_extract_words_filters_numbers_and_punctuation():
    words = list(extract_words("(131)I-Macroaggregated Albumin 5mg, well-being"))
    # Hyphenated runs of letters are kept as a single token.
    assert "I-Macroaggregated" in words
    assert "Albumin" in words
    assert "well-being" in words
    # No digits, single letters or empty tokens.
    assert all(not any(ch.isdigit() for ch in w) for w in words)
    assert "I" not in words


def test_extract_words_drops_amino_acid_sequences():
    # Hyphenated runs of single letters are biological sequences, not words.
    assert list(extract_words("A-T")) == []
    assert list(extract_words("A-T-G-G")) == []
    assert list(extract_words("a-t-a-t-t-t-g-g-c-g-a-t-g-g-c-t-t-c")) == []
    # Real hyphenated words and mixed tokens are still kept.
    assert "well-being" in list(extract_words("well-being"))
    assert "I-Macroaggregated" in list(extract_words("I-Macroaggregated"))


def test_extract_words_drops_peptide_sequences():
    # Three-letter amino acid sequences (and capped/modified variants) are
    # chemical notations, not words, so they must be dropped.
    assert list(extract_words("Val-Leu-Asn-Tyr-Tyr-Val-Trp")) == []
    assert list(extract_words("Val-Leu-Gly-OH")) == []
    assert list(extract_words("Val-Lys")) == []
    assert list(extract_words("Val-Lys-PABC-DOX")) == []
    assert list(extract_words("Val-MeArg-Gly-Asp")) == []
    assert list(extract_words("Val-leu")) == []  # lower-case variant
    # Real hyphenated words with at most one amino-acid-looking component stay.
    assert "well-being" in list(extract_words("well-being"))
    assert "I-Macroaggregated" in list(extract_words("I-Macroaggregated"))
    assert "Pro-American" in list(extract_words("Pro-American"))


def test_extract_words_drops_spelled_out_peptide_names():
    # Spelled-out peptide / acyl chemical names are not words.
    assert list(extract_words("glutamyl-leucyl-aspartyl-leucyl-alanyl-valyl-glutamyl-phenylalanine")) == []
    assert list(extract_words("glutamyl-lysyl")) == []
    assert list(extract_words("glutamyl-phenylalanine")) == []
    assert list(extract_words("glutamyl-methionine")) == []
    assert list(extract_words("glutamyl-tRNA")) == []
    assert list(extract_words("GLUTAMYL-L-TYROSYL-L-PROLYL")) == []
    assert list(extract_words("glutamyl-L-tyrosyl-L-cysteinyl-L-cysteinyl-L-asparaginyl")) == []
    # Full amino-acid names on their own remain valid dictionary words.
    assert "phenylalanine" in list(extract_words("phenylalanine"))
    assert "glutamyl" in list(extract_words("glutamyl"))
    # Ordinary hyphenated words are still kept.
    assert "well-being" in list(extract_words("well-being"))
    assert "Pro-American" in list(extract_words("Pro-American"))
    assert "I-Macroaggregated" in list(extract_words("I-Macroaggregated"))


def test_collapse_hyphenated_words():
    words = {
        # Repetitive hyphenated compounds: parts are independently coverable, so
        # they should be dropped (Hunspell accepts them via BREAK).
        "cytokine-induced",
        "Cytokine-Induced",
        "Cytokine-associated",
        "Cytokine-Cytokine",
        # Standalone medical words that cover the parts above.
        "cytokine",
        # A hyphenated word whose second part is NOT covered anywhere -> kept.
        "I-Macroaggregated",
    }
    # "induced" / "associated" come from the base (en_US) reference.
    reference = {"induced", "associated"}
    result = collapse_hyphenated_words(words, reference)
    assert "cytokine-induced" not in result
    assert "Cytokine-Induced" not in result
    assert "Cytokine-associated" not in result
    assert "Cytokine-Cytokine" not in result
    # Standalone roots and un-coverable hyphenated words remain.
    assert "cytokine" in result
    assert "I-Macroaggregated" in result


def test_collapse_hyphenated_words_no_reference():
    # Without a reference set, parts must be covered by standalone medical words.
    words = {"surgically-placed", "surgically", "placed", "hemoglobinuria-associated"}
    result = collapse_hyphenated_words(words)
    assert "surgically-placed" not in result  # both parts present standalone
    assert "surgically" in result
    assert "placed" in result
    # "associated" is not present standalone, so this one is kept.
    assert "hemoglobinuria-associated" in result


def test_build_word_set_skips_irrelevant():
    concepts = [parse_line(SAMPLE), parse_line(FRENCH), parse_line(SUPPRESSED)]
    # included_sab=set() accepts all sources so this exercises language/suppress.
    words = build_word_set(concepts, included_sab=set())
    assert "Albumin" in words
    assert "Albumine" not in words  # French, filtered out
    assert "Obsoleteword" not in words  # suppressed


def test_build_word_set_applies_sab_filter():
    # With an allow-list, only words from included sources survive. SNOMEDCT_US
    # is a licensed source, so it is added explicitly here to exercise the
    # keep-branch alongside the dropped sources.
    concepts = [parse_line(SNOMED_SAMPLE), parse_line(NCBI_SPECIES), parse_line(SAMPLE)]
    words = build_word_set(concepts, included_sab=FREE_SAB | LICENSED_SAB)
    assert "Atrial" in words and "fibrillation" in words  # SNOMEDCT_US, kept
    assert "Escherichia" not in words  # NCBI species name, dropped
    assert "coli" not in words
    assert "Albumin" not in words  # MSH, not in the default allow-list


def test_load_excluded_cuis_reads_organism_tuis():
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "MRSTY.RRF")
        with open(path, "w", encoding="utf-8", newline="\n") as handle:
            handle.write("C0000001|T007|A1|Bacterium|AT1|256|\n")   # organism
            handle.write("C0000002|T047|A2|Disease|AT2|256|\n")     # not organism
            handle.write("C0000003|T005|A3|Virus|AT3|256|\n")       # organism
        cuis = load_excluded_cuis(path)
        assert cuis == {"C0000001", "C0000003"}


def test_load_excluded_cuis_missing_file():
    assert load_excluded_cuis(os.path.join(tempfile.gettempdir(), "no_mrsty.rrf")) == set()


def test_build_word_set_excludes_organism_cuis():
    # The SNOMED clinical concept is kept; an organism concept (same allowed
    # source, SNOMEDCT_US) is dropped purely because its CUI is excluded.
    organism = (
        "C0085496|ENG|P|L0085496|PF|S0085496|Y|A00009999|||3092008|SNOMEDCT_US|PT|"
        "3092008|Bacillus cereus|0|N|256|"
    )
    concepts = [parse_line(SNOMED_SAMPLE), parse_line(organism)]
    words = build_word_set(
        concepts,
        included_sab=FREE_SAB | LICENSED_SAB,
        exclude_cuis={"C0085496"},
    )
    assert "fibrillation" in words            # clinical concept kept
    assert "Bacillus" not in words            # organism CUI dropped
    assert "cereus" not in words


def test_organism_tuis_cover_expected_kingdoms():
    # Sanity: bacteria, fungi and viruses are all in the excluded set.
    assert {"T007", "T004", "T005"} <= ORGANISM_TUIS


def test_load_reference_words_strips_flags_and_count():
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "ref.dic")
        with open(path, "w", encoding="utf-8", newline="\n") as handle:
            handle.write("3\n")  # leading count line
            handle.write("albumin/SM\n")
            handle.write("running po:verb\n")
            handle.write("Crohn's\n")
        ref = load_reference_words(path)
        # Count line ignored; flags and morphological data stripped; lower-cased.
        assert "albumin" in ref
        assert "running" in ref
        assert "crohn's" in ref
        assert "3" not in ref


def test_load_reference_words_missing_file():
    assert load_reference_words(os.path.join(tempfile.gettempdir(), "does_not_exist.dic")) == set()


def test_remove_reference_words_case_insensitive():
    words = {"Albumin", "metformin", "troponin"}
    reference = {"albumin"}  # already in en_US (lower-cased)
    result = remove_reference_words(words, reference)
    assert "Albumin" not in result  # removed despite different casing
    assert "metformin" in result
    assert "troponin" in result


def test_remove_reference_words_empty_reference_keeps_all():
    words = {"metformin", "troponin"}
    assert remove_reference_words(words, set()) == words


def test_parse_affix_rules_reads_cross_and_payload():
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "ref.aff")
        with open(path, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(AFF_SAMPLE)
        rules = parse_affix_rules(path)
        assert set("UAGPS").issubset(rules)
        # Header line itself must not become a rule (4-field headers are skipped).
        assert all(r.add or r.strip is not None for r in rules["S"])
        # Cross-product flag is taken from the class header.
        assert all(r.cross for r in rules["U"])
        # "0" strip/add normalised to empty string.
        u_rule = rules["U"][0]
        assert u_rule.strip == "" and u_rule.add == "un"


def test_expand_word_generates_derived_forms():
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "ref.aff")
        with open(path, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(AFF_SAMPLE)
        rules = parse_affix_rules(path)
        # willing/UPY-style: prefix "un" + suffix "ness" cross-product.
        forms = expand_word("willing", "UP", rules)
        assert {"willing", "unwilling", "willingness", "unwillingness"} <= forms
        # wind/UASG-style: prefixes + "-ing" suffix.
        forms = expand_word("wind", "UAG", rules)
        assert {"wind", "unwind", "rewind", "winding", "unwinding", "rewinding"} <= forms
        # A word with no flags only yields itself.
        assert expand_word("metformin", "", rules) == {"metformin"}


def test_load_reference_words_expanded_removes_derived_noise():
    with tempfile.TemporaryDirectory() as tmp:
        dic = os.path.join(tmp, "ref.dic")
        aff = os.path.join(tmp, "ref.aff")
        with open(aff, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(AFF_SAMPLE)
        with open(dic, "w", encoding="utf-8", newline="\n") as handle:
            handle.write("1\n")
            handle.write("willing/UP\n")
        ref = load_reference_words_expanded(dic, aff)
        # Derived forms are now part of the reference set (lower-cased)...
        assert {"willing", "unwilling", "willingness", "unwillingness"} <= ref
        # ...so they are removed from a medical list case-insensitively.
        medical = {"Unwilling", "Unwillingness", "metformin"}
        result = remove_reference_words(medical, ref)
        assert result == {"metformin"}


def test_load_reference_words_expanded_falls_back_without_aff():
    with tempfile.TemporaryDirectory() as tmp:
        dic = os.path.join(tmp, "ref.dic")
        with open(dic, "w", encoding="utf-8", newline="\n") as handle:
            handle.write("1\n")
            handle.write("willing/UP\n")
        # Missing/empty aff: only the bare head word is loaded.
        ref = load_reference_words_expanded(dic, os.path.join(tmp, "missing.aff"))
        assert ref == {"willing"}


def test_collapse_case_variants_keeps_lowercase_root():
    words = {
        # Triplet that collapses onto the lower-case form.
        "cytokine", "Cytokine", "CYTOKINE",
        # Pair with no lower-case form: keep the Title-case (fewest capitals),
        # not the ALL-CAPS variant.
        "Tylenol", "TYLENOL",
        # All-caps only (no lower / title form): kept as-is.
        "MRSA",
        # Unique lower-case word.
        "metformin",
    }
    result = collapse_case_variants(words)
    assert "cytokine" in result
    assert "Cytokine" not in result and "CYTOKINE" not in result
    assert "Tylenol" in result and "TYLENOL" not in result
    assert "MRSA" in result
    assert "metformin" in result
    # Each lower-cased word appears exactly once.
    assert len(result) == len({w.lower() for w in result})


def test_ensure_must_have_terms_readds_missing():
    # "pneumonia" is missing and not covered by the reference -> re-added.
    # "edema" is covered by the (base dictionary) reference -> not re-added.
    # "metformin" already present -> untouched.
    words = {"metformin"}
    must_have = {"pneumonia", "edema", "metformin"}
    reference = {"edema"}
    result = ensure_must_have_terms(words, must_have=must_have, reference=reference)
    assert "pneumonia" in result  # re-added
    assert "edema" not in result  # covered by en_US, intentionally not re-added
    assert "metformin" in result  # already present


def test_ensure_must_have_terms_case_insensitive_present():
    # A capitalised variant already present counts as present (no duplicate).
    words = {"Pneumonia"}
    result = ensure_must_have_terms(words, must_have={"pneumonia"}, reference=set())
    assert "pneumonia" not in result  # not re-added as a separate lower-case form
    assert result == {"Pneumonia"}


def test_default_must_have_terms_are_extractable_words():
    # Every curated must-have term must survive tokenisation (i.e. be a valid
    # single word), so the safety net can always re-add it.
    for term in MUST_HAVE_TERMS:
        assert list(extract_words(term)) == [term], term


def test_parse_included_sab():
    assert _parse_included_sab(None) is None          # use default INCLUDED_SAB
    assert _parse_included_sab("all") == set()         # accept every source
    assert _parse_included_sab("") == set()
    assert _parse_included_sab("MSH, NCI ,RXNORM") == {"MSH", "NCI", "RXNORM"}
    # The default allow-list excludes the taxonomy source.
    assert "NCBI" not in INCLUDED_SAB
    # The default is the freely redistributable set; the restrictively licensed
    # sources (SNOMED CT, MedDRA) are opt-in only via --include-licensed.
    assert INCLUDED_SAB == FREE_SAB
    assert "RXNORM" in INCLUDED_SAB
    assert "SNOMEDCT_US" not in INCLUDED_SAB
    assert "SNOMEDCT_US" in LICENSED_SAB
    assert "MDR" in LICENSED_SAB
    # Free and licensed sets must not overlap.
    assert FREE_SAB.isdisjoint(LICENSED_SAB)


def test_write_dictionary_format():
    with tempfile.TemporaryDirectory() as tmp:
        dic_path, aff_path = write_dictionary(["beta", "Alpha", "alpha"], output_dir=tmp)
        assert os.path.exists(dic_path)
        assert os.path.exists(aff_path)
        lines = open(dic_path, encoding="utf-8").read().splitlines()
        # First line is the word count.
        assert lines[0] == str(len(lines) - 1)
        # De-duplicated and sorted (case-insensitive primary order).
        assert lines[1:] == ["Alpha", "alpha", "beta"]
        aff = open(aff_path, encoding="utf-8").read()
        assert aff.startswith("SET UTF-8")
        assert "SFX S" in aff


def test_write_dictionary_emits_affix_flags():
    with tempfile.TemporaryDirectory() as tmp:
        dic_path, _ = write_dictionary(
            ["millilitre", "sepsis"],
            output_dir=tmp,
            flags={"millilitre": "S"},
        )
        lines = open(dic_path, encoding="utf-8").read().splitlines()
        assert lines[1:] == ["millilitre/S", "sepsis"]
        # The flag is included in the count line as a normal entry.
        assert lines[0] == "2"


def test_medical_affix_rules_has_plural_suffix():
    rules = medical_affix_rules()
    assert "S" in rules
    assert all(rule.kind == "SFX" for rule in rules["S"])


def test_collapse_plurals_basic_pair():
    # The regular singular/plural pair collapses onto "millilitre/S"; the plural
    # entry is dropped and the singular is tagged with the S flag.
    words = {"millilitre", "millilitres", "sepsis"}
    remaining, flags = collapse_plurals(words)
    assert "millilitres" not in remaining
    assert "millilitre" in remaining
    assert flags == {"millilitre": "S"}
    # A word with no stored plural is untouched and unflagged.
    assert "sepsis" in remaining


def test_collapse_plurals_case_insensitive():
    # A capitalised plural ("Millimoles") collapses onto the lower-case singular
    # ("millimole"); Hunspell accepts the capitalised plural from the lower root.
    words = {"millimole", "Millimoles"}
    remaining, flags = collapse_plurals(words)
    assert "Millimoles" not in remaining
    assert remaining == {"millimole"}
    assert flags == {"millimole": "S"}


def test_collapse_plurals_es_and_ies():
    # "-es" after a sibilant and "y"->"ies" plurals are recognised too.
    words = {"reflex", "reflexes", "biopsy", "biopsies"}
    remaining, flags = collapse_plurals(words)
    assert remaining == {"reflex", "biopsy"}
    assert flags == {"reflex": "S", "biopsy": "S"}


def test_collapse_plurals_keeps_unpaired_plural():
    # A plural with no stored singular survives (we never invent the singular).
    words = {"forceps", "adnexa"}
    remaining, flags = collapse_plurals(words)
    assert remaining == {"forceps", "adnexa"}
    assert flags == {}


def test_medical_affix_rules_has_genitive_suffix():
    rules = medical_affix_rules()
    assert "M" in rules
    suffixes = rules["M"]
    assert suffixes and all(rule.kind == "SFX" for rule in suffixes)
    # The single rule appends "'s" with no condition/strip.
    rule = suffixes[0]
    assert rule.add == "'s" and rule.strip == ""


def test_collapse_genitives_basic_pair():
    # The base/genitive pair collapses onto "Towne/M"; the "'s" entry is dropped
    # and the base word is tagged with the M flag.
    words = {"Towne", "Towne's", "sepsis"}
    remaining, flags = collapse_genitives(words)
    assert "Towne's" not in remaining
    assert "Towne" in remaining
    assert flags == {"Towne": "M"}
    # A word with no stored genitive is untouched and unflagged.
    assert "sepsis" in remaining


def test_collapse_genitives_case_insensitive():
    # A differently-cased genitive collapses onto the stored base word.
    words = {"tourette", "Tourette's"}
    remaining, flags = collapse_genitives(words)
    assert "Tourette's" not in remaining
    assert remaining == {"tourette"}
    assert flags == {"tourette": "M"}


def test_collapse_genitives_keeps_unpaired_genitive():
    # A "'s" form with no stored base survives (we never invent the base).
    words = {"Down's", "sepsis"}
    remaining, flags = collapse_genitives(words)
    assert remaining == {"Down's", "sepsis"}
    assert flags == {}


def test_plural_and_genitive_flags_combine():
    # Mirrors the issue example: Tourette has both a plural (collapsed to /S) and
    # a genitive (collapsed to /M); the two passes' flags merge into "Tourette/SM".
    words = {"Tourette", "Tourettes", "Tourette's", "Towne", "Towne's"}
    words, plural_flags = collapse_plurals(words)
    assert "Tourettes" not in words
    words, genitive_flags = collapse_genitives(words)
    assert "Tourette's" not in words and "Towne's" not in words
    head_flags: dict[str, str] = {}
    for word, flag in plural_flags.items():
        head_flags[word] = head_flags.get(word, "") + flag
    for word, flag in genitive_flags.items():
        head_flags[word] = head_flags.get(word, "") + flag
    assert head_flags["Tourette"] == "SM"
    assert head_flags["Towne"] == "M"


def test_collapse_genitives_then_write_emits_M_flag():
    words = {"Towne", "Towne's"}
    remaining, flags = collapse_genitives(words)
    with tempfile.TemporaryDirectory() as tmp:
        dic_path, _ = write_dictionary(remaining, output_dir=tmp, flags=flags)
        lines = open(dic_path, encoding="utf-8").read().splitlines()
        assert lines[1:] == ["Towne/M"]
