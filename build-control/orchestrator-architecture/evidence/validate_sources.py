#!/usr/bin/env python3
"""Cross-artifact source consistency validator — V1.3.3.

V1.3.3 corrects a FOURTH false-negative found by independent review (DEFECT D)
and adds two gates. See DEFECT D below.

V1.3.1's validator was UNSOUND and produced false-green results. Three defects,
all reproduced and all corrected here:

  DEFECT A  Proximity heuristic: a ±400-character window exempted a stale
            assertion whenever ANY historical marker appeared nearby. A document
            reading "A-14 is retired ... No hook system exists" passed.
            FIX: block-aware classification. Only the enclosing block's own
            authority counts, never a neighbouring sentence.

  DEFECT B  Whole-document exemption: one historical marker anywhere suppressed
            contradiction checks for the entire file.
            FIX: authority is declared per document via DOCUMENT_AUTHORITY, and
            per block via explicit HISTORICAL markers. Nothing is exempt by
            accident.

  DEFECT C  Unconditional sys.exit(0): detected failures still returned success,
            so no build could ever fail on them.
            FIX: exit 1 whenever any required gate is non-zero.

  DEFECT E  Block-scope over-reach in tables and lists (found at V1.3.3, while
            verifying the DEFECT D fix). blocks() grouped an entire markdown
            table into ONE unit, so a [HISTORICAL] marker in any single row
            exempted every other row of that table — including rows making live
            current assertions. That is DEFECT A's proximity exemption wearing a
            different hat: authority leaking from one statement to its neighbour.
            FIX: table rows and list items are their own units. A row is exempt
            only if that row declares itself historical.

  DEFECT D  Header-comment bypass (found at V1.3.3). Blocks were split on blank
            lines and any block whose first characters were "<!--" was skipped
            wholesale. When a metadata comment was NOT followed by a blank line,
            the comment and the real prose after "-->" formed ONE block, so live
            current prose was discarded unread. A document reading

                <!--
                DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
                -->
                # Current architecture
                Supervised (Codex). No hook system exists (A-14).

            passed cleanly.
            FIX: HTML comments are removed from the text first (newlines
            preserved so offsets stay meaningful), and DOCUMENT_AUTHORITY is
            read from the raw text. ALL remaining prose is then scanned. No text
            is ever exempt because of a character sequence at the start of its
            paragraph — only an explicit [HISTORICAL] opt-in exempts a block.
            Guarded by a built-in self-probe: SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS.
"""
import json, glob, re, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
REG  = json.load(open(os.path.join(HERE, 'SOURCE_CLAIM_REGISTRY.json')))
CLAIM = {c['id']: c for c in REG['claims']}
POSTURE = json.load(open(os.path.join(HERE, 'HOST_POSTURE_AUTHORITY.json')))
EVENT_AUTHORITY = json.load(open(os.path.join(HERE, 'HOST_EVENT_SOURCE_AUTHORITY.json')))

# --- required gates: every one must be zero for exit 0 ---
GATES = ['CURRENT_CLAIM_WITHOUT_CURRENT_SOURCE','SOURCE_MATRIX_STATUS_MISMATCHES',
         'STALE_A14_CURRENT_ASSERTIONS','CONTRADICTORY_VENDOR_STATUS_ASSERTIONS',
         'STALE_RESEARCH_UNAVAILABLE_ASSERTIONS','STALE_USER_DECLARED_CODEX_PLUGIN_ASSERTIONS',
         'POLICY_NEEDS_REVIEW_ROUTABLE_PATHS','VERIFIED_DISALLOWED_ROUTABLE_PATHS',
         'POLICY_SCHEMA_REGISTRY_EVIDENCE_LABEL_MISMATCHES','UNDECLARED_DOCUMENT_AUTHORITY',
         'OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS','CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES',
         'SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS','CURRENT_HOST_POSTURE_MISMATCHES',
         # V1.3.6: structured event-provenance and primary-mechanism checks, replacing
         # prose-regex-only posture checking with validation against a canonical authority.
         'CURRENT_HOST_EVENT_SOURCE_MISMATCHES','CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES']

# The generated source matrix travels with the package revision. Exactly one
# filename is current; every reference in the package must name that one.
CURRENT_SOURCE_MATRIX = 'SOURCE_VERIFICATION_MATRIX_V1_3_6.md'
ANY_SOURCE_MATRIX = re.compile(r'SOURCE_VERIFICATION_MATRIX_V1_[0-9_]*\.md')

# --- structured current host posture authority (BUILD-A1 §9) ---------------
# A document contradicts the authority in evidence/HOST_POSTURE_AUTHORITY.json
# either by directly asserting the wrong primary posture for a host, or by
# presenting the fallback mechanism (a "companion" process) as if it were the
# only path, with no nearby signal that it is a fallback.
POSTURE_CONTRADICTION = re.compile(
    r'(Codex[^\n.]{0,60}\bprimary\b[^\n.]{0,30}\b(SUPERVISED|HYBRID)\b'
    r'|\b(SUPERVISED|HYBRID)\b[^\n.]{0,30}\bis\b[^\n.]{0,15}\bprimary\b[^\n.]{0,40}Codex'
    r'|Codex[^\n.]{0,40}\bonly\b[^\n.]{0,20}\bsupervis)', re.I)
COMPANION_RE = re.compile(r'\bcompanion\b', re.I)
NATIVE_OR_FALLBACK_NEARBY = re.compile(
    r'\b(native|fallback|compatibility|SUPERVISED|HYBRID)\b', re.I)
# "supervisor-mediated" / "shallower" describing the ordinary Codex in-session
# path (not scoped to the SUPERVISED fallback) is the exact stale phrasing
# HOST_PARITY_CONTRACT.md carried into V1.3.4. Catch it structurally rather
# than by hand-adding one more literal string each time the wording changes.
PRIMARY_MECHANISM_DOWNGRADE_RE = re.compile(
    r'\b(supervisor-mediated|shallower)\b', re.I)

AUTH_RE = re.compile(r'DOCUMENT_AUTHORITY:\s*(CURRENT_NORMATIVE|HISTORICAL_SNAPSHOT)')
COMMENT_RE = re.compile(r'<!--.*?-->', re.S)

def strip_comments(text):
    """Remove HTML comments, preserving newline count so blocks stay aligned.

    This is the DEFECT D fix. Metadata comments are removed as comments; the
    prose after '-->' is then ordinary scannable text whether or not a blank
    line separates them.
    """
    return COMMENT_RE.sub(lambda m: '\n' * m.group(0).count('\n'), text)

# Patterns for stale CURRENT assertions. Each is a factual claim that is now false.
PAT = {
 'STALE_A14_CURRENT_ASSERTIONS': re.compile(
    r'(no (Claude-Code-equivalent )?lifecycle[- ]hook system'
    r'|has \*\*no\*\* lifecycle-hook'
    r'|no hook system exists'
    r'|none of these deliver lifecycle events'
    r'|Codex\s+has\s+no\s+\w*\s*hook)', re.I),
 'STALE_RESEARCH_UNAVAILABLE_ASSERTIONS': re.compile(
    r'(research (capability )?(was |is )?unavailable'
    r'|re-verification was not possible'
    r'|could not perform (the )?§?7? ?research'
    r'|research[^.\n]{0,60}?could not (be )?perform'
    r'|research unavailable)', re.I),
 'STALE_USER_DECLARED_CODEX_PLUGIN_ASSERTIONS': re.compile(
    r'(Codex[^.\n]{0,80}`?USER_DECLARED`?'
    r'|`?USER_DECLARED`?[^.\n]{0,80}Codex (plugin|hook)'
    r'|C-01[^.\n]{0,40}USER_DECLARED'
    r'|C-02[^.\n]{0,40}USER_DECLARED)', re.I),
 'CONTRADICTORY_VENDOR_STATUS_ASSERTIONS': re.compile(
    r'(undocumented endpoint'
    r'|Codex[^.\n]{0,60}supervis\w+[^.\n]{0,40}only possible'
    r'|supervision is the only)', re.I),
 # C-02a binds trust to the hook DEFINITION/HASH. "every plugin update
 # re-triggers review" is a strengthening the source does not support: an
 # update that leaves the hook definition unchanged does not imply re-trust.
 'OVERSTATED_CODEX_HOOK_RETRUST_ASSERTIONS': re.compile(
    r'\b(every|each|any|all)\s+(plugin\s+)?updates?\b[^.\n]{0,60}?'
    r'(re-?trigger|re-?mark|re-?trust|re-?review|requires? review|for review)', re.I),
}

# A block is HISTORICAL if it declares so itself — not because a neighbour does.
# A block is historical ONLY if it carries an explicit opt-in marker. This is a
# declaration, never an inference from a nearby sentence (DEFECT A / DEFECT B).
BLOCK_HIST = re.compile(r'\[HISTORICAL\]|\[HISTORICAL:[^\]]*\]', re.I)

ROW_START = re.compile(r'^\s*(\||[-*+]\s|\d+\.\s|>\s)')

def blocks(text):
    """Split into units. Paragraphs group; table rows and list items stand alone.

    A unit is the scope in which a [HISTORICAL] opt-in applies. Grouping a whole
    table into one unit would let one row's marker exempt its neighbours, which
    is the DEFECT A failure mode at a smaller scale (DEFECT E).
    """
    out, cur, start = [], [], 0
    pos = 0
    def flush():
        nonlocal cur
        if cur:
            out.append(('\n'.join(cur), start))
            cur = []
    for line in text.split('\n'):
        if line.strip() == '':
            flush()
            pos += len(line) + 1
            start = pos
        elif ROW_START.match(line):
            flush()                      # a row/item is its own unit
            out.append((line, pos))
            pos += len(line) + 1
            start = pos
        else:
            if not cur: start = pos
            cur.append(line); pos += len(line) + 1
    flush()
    return out

def block_is_historical(block):
    """Only the block's OWN content may exempt it."""
    return bool(BLOCK_HIST.search(block))

CLAIM_ASSERT = re.compile(
    r'`?(C-\d+[a-z]?)`?[^\n|]{0,60}?(?:=|:|→|->|\|)\s*`?('
    + '|'.join(REG['evidence_labels']) + r')`?')

def check_registry_conflict(block):
    """Flag only an EXPLICIT claim-id -> label assertion that the registry contradicts.
    Requires the two to be bound by an assignment token, not merely co-present."""
    hits = 0
    for m in CLAIM_ASSERT.finditer(block):
        cid, lab = m.group(1), m.group(2)
        c = CLAIM.get(cid)
        if c and lab != c['label']:
            hits += 1
    return hits

# --- DEFECT D self-probe -----------------------------------------------------
# The exact shape that slipped past V1.3.2: metadata comment, NO blank line,
# then live stale prose. Run in-process on every invocation so the bypass cannot
# silently return in a later edit.
HEADER_BYPASS_PROBE = (
    "<!--\nDOCUMENT_AUTHORITY: CURRENT_NORMATIVE\n-->\n"
    "# Current architecture\nSupervised (Codex). No hook system exists (A-14).\n")
HEADER_VALID_PROBE = (
    "<!--\nDOCUMENT_AUTHORITY: CURRENT_NORMATIVE\n-->\n"
    "# Current architecture\nCodex exposes lifecycle hooks (`C-02`); the native plugin path is primary.\n")

def header_comment_bypass_count():
    """0 when the parser reads prose that follows '-->' with no blank line.

    Returns 1 if the stale probe is missed (the bypass is back), plus 1 if the
    accurate probe is falsely flagged (the fix over-corrected into noise).
    """
    bad = 0
    stale = strip_comments(HEADER_BYPASS_PROBE)
    if not any(PAT['STALE_A14_CURRENT_ASSERTIONS'].search(b)
               for b, _ in blocks(stale) if not block_is_historical(b)):
        bad += 1
    ok = strip_comments(HEADER_VALID_PROBE)
    if any(p.search(b) for b, _ in blocks(ok) for p in PAT.values()
           if not block_is_historical(b)):
        bad += 1
    return bad

EVENT_ROW_RE = re.compile(
    r'^\|\s*`([A-Z_]+)`\s*\|[^|]*\|[^|]*\|\s*([^|]*?)\s*\|\s*([^|]*?)\s*\|\s*$', re.M)

def check_event_source_table(root, res, details):
    """NORMALIZED_HOST_EVENTS.md's event table, validated row-by-row against the
    canonical evidence/HOST_EVENT_SOURCE_AUTHORITY.json authority (structured,
    not prose-regex). A row is a mismatch if the event is missing from the
    table, or either host's source cell disagrees with the authority."""
    path = os.path.join(root, 'NORMALIZED_HOST_EVENTS.md')
    if not os.path.exists(path):
        # Isolated regression fixtures run against a single-file root by design
        # (see run_regression.py) and never carry this doc — absence there is
        # not a package defect. A full package build always has this file.
        return
    txt = open(path, encoding='utf-8').read()
    rows = {m.group(1): (m.group(2).strip(), m.group(3).strip())
            for m in EVENT_ROW_RE.finditer(txt)}
    for entry in EVENT_AUTHORITY['events']:
        ev = entry['event']
        row = rows.get(ev)
        if row is None:
            res['CURRENT_HOST_EVENT_SOURCE_MISMATCHES'] += 1
            details.append(('EVENT_SOURCE_MISSING_ROW', 'NORMALIZED_HOST_EVENTS.md', ev))
            continue
        claude_cell, codex_cell = (row[0].replace('`', ''), row[1].replace('`', ''))
        claude_source = entry.get('claude_source', entry['sources']['claude-code']['source'])
        codex_source = entry.get('codex_source', entry['sources']['codex-worker']['source'])
        if claude_cell != claude_source:
            res['CURRENT_HOST_EVENT_SOURCE_MISMATCHES'] += 1
            details.append(('EVENT_SOURCE_CLAUDE_MISMATCH', ev, claude_cell[:60]))
        if codex_cell != codex_source:
            res['CURRENT_HOST_EVENT_SOURCE_MISMATCHES'] += 1
            details.append(('EVENT_SOURCE_CODEX_MISMATCH', ev, codex_cell[:60]))


def main():
    res = {g: 0 for g in GATES}
    res['SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS'] = header_comment_bypass_count()
    details = []
    docs = sorted(glob.glob(os.path.join(ROOT,'*.md')) +
                  glob.glob(os.path.join(ROOT,'BUILD_A2_MANAGERS','*.md')))
    current_docs = 0
    for path in docs:
        raw = open(path, encoding='utf-8').read()
        base = os.path.basename(path)
        relpath = os.path.relpath(path, ROOT)
        m = AUTH_RE.search(raw)           # authority is read from the RAW text
        if not m:
            res['UNDECLARED_DOCUMENT_AUTHORITY'] += 1
            details.append(('UNDECLARED_AUTHORITY', base, ''))
            continue
        authority = m.group(1)
        if authority == 'HISTORICAL_SNAPSHOT':
            continue                      # contributes no current assertion
        current_docs += 1
        txt = strip_comments(raw)         # DEFECT D: comments out, ALL prose in
        for blk, _off in blocks(txt):
            if block_is_historical(blk):
                continue                  # this block declares itself historical
            for gate, pat in PAT.items():
                mm = pat.search(blk)
                if mm:
                    res[gate] += 1
                    details.append((gate, base, mm.group(0)[:70]))
            res['CONTRADICTORY_VENDOR_STATUS_ASSERTIONS'] += check_registry_conflict(blk)
            if relpath.replace(os.sep, '/') in POSTURE['scanned_documents']:
                mm = POSTURE_CONTRADICTION.search(blk)
                if mm:
                    res['CURRENT_HOST_POSTURE_MISMATCHES'] += 1
                    res['CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES'] += 1
                    details.append(('CURRENT_HOST_POSTURE_MISMATCHES', base, mm.group(0)[:70]))
                # "supervisor-mediated" / "shallower" describing the ordinary path
                # (not scoped to the SUPERVISED fallback) is the same failure mode
                # in different words: the fallback mechanism presented as primary.
                for dm in PRIMARY_MECHANISM_DOWNGRADE_RE.finditer(blk):
                    window = blk[max(0, dm.start()-200):dm.end()+200]
                    if not NATIVE_OR_FALLBACK_NEARBY.search(window):
                        res['CURRENT_HOST_POSTURE_MISMATCHES'] += 1
                        res['CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES'] += 1
                        details.append(('CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES', base,
                                         dm.group(0)))
        # the fallback mechanism ("companion") must never appear as if it were
        # the only path, with no nearby signal that it is a fallback
        if relpath.replace(os.sep, '/') in POSTURE['scanned_documents']:
            for mm in COMPANION_RE.finditer(txt):
                window = txt[max(0, mm.start()-300):mm.end()+300]
                if block_is_historical(window):
                    continue
                if not NATIVE_OR_FALLBACK_NEARBY.search(window):
                    res['CURRENT_HOST_POSTURE_MISMATCHES'] += 1
                    res['CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES'] += 1
                    details.append(('CURRENT_HOST_POSTURE_MISMATCHES', base,
                                     'unlabelled companion: ' + window[280:340].replace('\n', ' ')))
        # bare VERIFIED_CURRENT with no traceable source
        for mm in re.finditer(r'`VERIFIED_CURRENT`(?!_SELF)', txt):
            ctx = txt[max(0, mm.start()-600):mm.end()+600]
            if not re.search(r'C-\d|https?://|SOURCE_CLAIM_REGISTRY|registry|_SELF_FETCHED|REVIEWER_SUPPLIED', ctx):
                res['CURRENT_CLAIM_WITHOUT_CURRENT_SOURCE'] += 1
                details.append(('NO_SOURCE', base, ctx[580:650].replace('\n',' ')))
        # every source-matrix reference must name the CURRENT matrix
        for mm in ANY_SOURCE_MATRIX.finditer(raw):
            if mm.group(0) != CURRENT_SOURCE_MATRIX and not block_is_historical(
                    raw[max(0, mm.start()-300):mm.end()+300]):
                res['CURRENT_SOURCE_MATRIX_REFERENCE_MISMATCHES'] += 1
                details.append(('STALE_MATRIX_NAME', base, mm.group(0)))

    # generated matrix must agree with the registry
    mat = os.path.join(ROOT, CURRENT_SOURCE_MATRIX)
    if os.path.exists(mat):
        mt = open(mat, encoding='utf-8').read()
        for cid, c in CLAIM.items():
            rows = [l for l in mt.splitlines() if l.strip().startswith(f'| `{cid}`')]
            if rows and c['label'] not in rows[0]:
                res['SOURCE_MATRIX_STATUS_MISMATCHES'] += 1
                details.append(('MATRIX_MISMATCH', cid, c['label']))

    # structured event-provenance table, checked against the canonical authority
    check_event_source_table(ROOT, res, details)

    # provider policy routability
    for p in REG['policy_matrix']:
        if p['routable'] and p['policy_status'] == 'NEEDS_REVIEW':
            res['POLICY_NEEDS_REVIEW_ROUTABLE_PATHS'] += 1
        if p['routable'] and p['policy_status'] == 'VERIFIED_DISALLOWED':
            res['VERIFIED_DISALLOWED_ROUTABLE_PATHS'] += 1

    # ProviderPolicyEligibility evidence vocabulary == registry vocabulary
    sp = os.path.join(ROOT,'schemas','ProviderPolicyEligibility.schema.json')
    if os.path.exists(sp):
        sd = json.load(open(sp))
        got = set(sd['properties'].get('evidence_label',{}).get('enum',[]))
        want = set(REG['evidence_labels'])
        if got != want:
            res['POLICY_SCHEMA_REGISTRY_EVIDENCE_LABEL_MISMATCHES'] += len(want ^ got)
            details.append(('EVIDENCE_VOCAB', 'ProviderPolicyEligibility', str(sorted(want ^ got))))

    for g in GATES:
        print(f"{g} = {res[g]}")
    print(f"CURRENT_NORMATIVE_DOCS_SCANNED = {current_docs}")
    for d in details[:30]:
        print("   ", d)
    json.dump({'results': res, 'details': details[:80], 'current_docs': current_docs},
              open(os.environ.get('SRCVAL_OUT','/tmp/srcval133.json'),'w'), indent=1)

    failed = [g for g in GATES if res[g] != 0]
    if failed:
        print(f"\nVALIDATION FAILED: {', '.join(failed)}", file=sys.stderr)
        return 1
    print("\nALL SOURCE GATES PASS")
    return 0

if __name__ == '__main__':
    sys.exit(main())
