
from dataclasses import dataclass
from typing import Optional

from enums import UMLSLanguageOfTerm, UMLSTermStatus, UMLSStringType, UMLSTermAbbreviation, UMLSSuppresssFlag


@dataclass
class MetathesaurusConcept:
    """Concept Names and Sources (File = MRCONSO.RRF)
    Every string or concept name in the Metathesaurus appears in this file, connected to its language, source vocabularies, and its concept identifier. The values of TS, STT, and ISPREF reflect the default order of precedence of vocabulary sources and term types in MRRANK.RRF

    Sample Records:
    - C0001175|ENG|P|L0001175|VO|S0010340|Y|A0019182||M0000245|D000163|MSH|PM|D000163|Acquired Immunodeficiency Syndromes|0|N||
    - C0001175|ENG|S|L0001842|PF|S0011877|N|A2878223|103840012|62479008||SNOMEDCT_US|PT|62479008|AIDS|9|N|2304|
    - C0001175|ENG|P|L0001175|VO|S0354232|Y|A2922342|103845019|62479008||SNOMEDCT_US|SY|62479008|Acquired immunodeficiency syndrome|9|N|2304|
    - C0001175|FRE|S|L0162173|PF|S0226654|Y|A27478989||M0000245|D000163|MSHFRE|ET|D000163|SIDA|3|N||
    - C0001175|RUS|S|L0904943|PF|S1108760|Y|A13488500||M0000245|D000163|MSHRUS|SY|D000163|SPID|3|N||
    """
    __tablename__ = '_MR_Concept'
    # Primary Key = AUI - "Unique identifier for atom"
    CUI: str
    """Unique identifier for concept"""
    LAT: UMLSLanguageOfTerm
    """Language of term"""
    TS: UMLSTermStatus
    """Term status"""
    LUI: str
    """Unique identifier for term"""
    STT: Optional[UMLSStringType]
    """String type"""
    SUI: str
    """Unique identifier for string"""
    ISPREF: bool
    """Atom status - preferred (Y) or not (N) for this string within this concept"""
    AUI: str
    """Unique identifier for atom - variable length field, 8 or 9 characters"""
    SAUI: Optional[str]
    """Source asserted atom identifier [optional]"""
    SCUI: Optional[str]
    """Source asserted concept identifier [optional]"""
    SDUI: Optional[str]
    """Source asserted descriptor identifier [optional]"""
    SAB: str
    """Abbreviated source name (SAB). Maximum field length is 20 alphanumeric characters. Two source abbreviations are assigned:
    - Root Source Abbreviation (RSAB) — short form, no version information, for example, AI/RHEUM, 1993, has an RSAB of "AIR"
    - Versioned Source Abbreviation (VSAB) — includes version information, for example, AI/RHEUM, 1993, has an VSAB of "AIR93"
    """
    TTY: UMLSTermAbbreviation
    """Abbreviation for term type in source vocabulary, for example PN (Metathesaurus Preferred Name) or CD (Clinical Drug). Possible values are listed on the Abbreviations Used in Data Elements page."""
    CODE: str
    """Most useful source asserted identifier (if the source vocabulary has more than one identifier), or a Metathesaurus-generated source entry identifier (if the source vocabulary has none)"""
    STR: str
    """String"""
    SRL: int
    """Source restriction level"""
    SUPPRESS: UMLSSuppresssFlag
    """Suppressible flag. Values = O, E, Y, or N
    O: All obsolete content, whether they are obsolesced by the source or by NLM. These will include all atoms having obsolete TTYs, and other atoms becoming obsolete that have not acquired an obsolete TTY (e.g. RxNorm SCDs no longer associated with current drugs, LNC atoms derived from obsolete LNC concepts).
    E: Non-obsolete content marked suppressible by an editor. These do not have a suppressible SAB/TTY combination.
    Y: Non-obsolete content deemed suppressible during inversion. These can be determined by a specific SAB/TTY combination explicitly listed in MRRANK.
    N: None of the above
    Default suppressibility as determined by NLM (i.e., no changes at the Suppressibility tab in MetamorphoSys) should be used by most users, but may not be suitable in some specialized applications. See the MetamorphoSys Help page for information on how to change the SAB/TTY suppressibility to suit your requirements. NLM strongly recommends that users not alter editor-assigned suppressibility, and MetamorphoSys cannot be used for this purpose.
    """
    CVF: Optional[int]
    """Content View Flag. Bit field used to flag rows included in Content View. This field is a varchar field to maximize the number of bits available for use."""
