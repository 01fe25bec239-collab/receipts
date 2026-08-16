#!/usr/bin/env python3
"""Package-structure validator — V1.3.3 (new).

V1.3.2 shipped derived counts that had been measured at some earlier moment and
then typed into PACKAGE_INDEX.md / PACKAGE_VALIDATION_REPORT.md /
FREEZE_READINESS_REPORT.md by hand. Independent review found them wrong by
exactly two in every archive dimension, while the reports simultaneously
asserted `DISPLAYED_DERIVED_COUNT_MISMATCHES = 0` "by construction". A count
that is asserted rather than computed is not derived, however it is labelled.

This module is the measurement pass. It computes every number the package
displays, and then checks the displayed numbers against what it computed.
`evidence/build_package.py` renders the reports FROM this module's output and
re-runs it against the finished ZIP.

Usage:
    validate_package.py [--root DIR] [--zip FILE] [--json OUT] [--no-display-check]

--zip makes the archive dimensions authoritative from the ZIP itself rather
than from a staging directory, which is the whole point of the V1.3.3
sequence: the thing measured must be the thing shipped.
"""
import argparse, hashlib, json, os, re, sys, zipfile
from collections import Counter, defaultdict

try:
    if os.environ.get('ORCHESTRATOR_DISABLE_JSONSCHEMA') == '1':
        raise ImportError('forced validation-backend absence probe')
    from jsonschema import Draft202012Validator
    from referencing import Registry, Resource
    SCHEMA_VALIDATOR_AVAILABLE = True
except Exception:
    Draft202012Validator = Registry = Resource = None
    SCHEMA_VALIDATOR_AVAILABLE = False

MANAGERS = ['BUILD-A2-ORCHESTRATION','BUILD-A2-MODEL-ROUTING','BUILD-A2-RUNTIME-ADAPTERS',
            'BUILD-A2-WORKSPACE-EXECUTION','BUILD-A2-REVIEW-INTEGRATION',
            'BUILD-A2-STATE-CONTEXT','BUILD-A2-HOST-INTEGRATION']

GATES = [
 # structure
 'BUILD_A2_COUNT_MISMATCH','BUILD_DAG_CYCLES','WAVE_ORDER_VIOLATIONS',
 'SCHEMA_INVALID','SCHEMA_OWNER_MISMATCH','VENDOR_ENUM_VIOLATIONS',
 'GRAPH_SCHEMA_FAILURES','PRECEDENCE_DAG_CYCLE_COUNT',
 'INVALID_GRAPH_EDGE_CLASS_COMBINATIONS_ACCEPTED','ADMISSION_FIXTURE_FAILURES',
 'FEATURE_ADMISSION_PROVIDER_OUTCOMES','FEATURE_ADMISSION_SAFETY_OUTCOMES',
 'CONTRACT_OWNER_COLLISIONS','PATH_OWNER_COLLISIONS',
 'MANAGER_OWNED_CONTRACT_SET_MISMATCHES','MANAGER_DUPLICATE_NORMATIVE_OWNERSHIP_SECTIONS',
 'SELF_OWNED_EXTERNAL_CONSUMPTIONS',
 # V1.3.4: path-ownership completeness, M0 classification, wave/contract truth
 'UNOWNED_REQUIRED_PATHS','AMBIGUOUS_REQUIRED_PATHS','REQUIRED_PATH_OWNER_MISMATCHES',
 'M0_SCHEMA_CLASSIFICATION_MISMATCHES','UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES',
 # V1.3.4: HostCapabilityReport schema coherence and doc/schema vocabulary
 'HOST_CAPABILITY_INVALID_STATE_ACCEPTED','HOST_CAPABILITY_VALID_STATE_REJECTED',
 'PLUGIN_INSTALLED_WITHOUT_SUPPORT_ACCEPTED',
 'HOOKS_CONFIGURED_WITHOUT_SUPPORT_ACCEPTED',
 'HOOKS_ENABLED_WITHOUT_SUPPORT_ACCEPTED',
 'HOOK_TRUST_REQUIRED_UNKNOWN_ACCEPTED',
 'HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES',
 # V1.3.6: explicit trust model, required-coverage gate, inactive_reason precedence
 'HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED',
 'HOST_CAPABILITY_INSUFFICIENT_COVERAGE_EMBEDDED_ACCEPTED',
 'HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED',
 # V1.3.6: freshness, honest unknowns, deterministic converse, authority schema
 'STALE_HOST_CAPABILITY_REPORT_EMBEDDED_ACCEPTED',
 'UNKNOWN_REASON_WITH_KNOWN_FAILURE_ACCEPTED',
 'HEALTHY_NATIVE_PATH_FALLBACK_WITHOUT_OVERRIDE_ACCEPTED',
 'MODE_SELECTION_STATE_MISMATCHES',
 'HOST_CAPABILITY_FRESHNESS_TRIGGER_MISMATCHES',
 'EVENT_AUTHORITY_SCHEMA_INVALID','EVENT_AUTHORITY_UNKNOWN_SOURCE_CLASSES',
 'EVENT_AUTHORITY_DUPLICATE_EVENTS','EVENT_AUTHORITY_MISSING_REQUIRED_HOST_SOURCES',
 'NORMALIZED_HOST_EVENT_TABLE_AUTHORITY_MISMATCHES',
 # V1.3.4: parity count/coverage derived from the canonical capability table
 'PARITY_DISPLAYED_COUNT_MISMATCHES','PARITY_CONFORMANCE_COVERAGE_MISSING',
 # V1.3.6: every INSTALL_MANIFEST path resolves to exactly one installation-map rule
 'INSTALL_MANIFEST_UNMAPPED_PATHS','INSTALLATION_MAP_AMBIGUOUS_PATHS','INSTALLATION_MAP_PATH_MISMATCHES',
 # cross-document truth
 'OPEN_QUESTION_STATUS_SOURCE_MISMATCHES','OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES',
 'TRACEABILITY_CURRENT_ROW_COUNT_MISMATCH','TRACEABILITY_STATUS_COUNT_MISMATCHES',
 'TRACEABILITY_STALE_RESOLVED_QUESTION_REFS','TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES',
 'CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS',
 # manifests and archive
 'MANIFEST_HASH_FAILURES','ZIP_SAFETY_VIOLATIONS',
 # V1.3.7: JSON Schema is a required freeze-validator dependency, never optional.
 'MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED','VALIDATION_DEPENDENCY_FAIL_OPEN',
 'FINAL_REPORT_PACKAGE_GATE_MISMATCHES',
 # the gate V1.3.2 claimed and did not enforce
 'DISPLAYED_DERIVED_COUNT_MISMATCHES',
]

REPORTS = ['PACKAGE_INDEX.md','PACKAGE_VALIDATION_REPORT.md','FREEZE_READINESS_REPORT.md']
CURRENT_EVIDENCE_LABELS = {'VERIFIED_CURRENT_SELF_FETCHED',
                           'REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE',
                           'DESIGN_DECISION','INDEPENDENT_VERIFIED'}
OPEN_EVIDENCE_LABELS = {'POLICY_NEEDS_REVIEW','UNVERIFIED'}


# ---------------------------------------------------------------- helpers
def read(root, rel):
    with open(os.path.join(root, rel), encoding='utf-8') as f:
        return f.read()

def table_rows(text, ncols):
    """Markdown table rows with exactly ncols cells, header/separator dropped."""
    out = []
    for line in text.splitlines():
        s = line.strip()
        if not s.startswith('|'):
            continue
        cells = [c.strip() for c in s.strip('|').split('|')]
        if len(cells) != ncols:
            continue
        if set(''.join(cells)) <= set('-: '):
            continue
        if cells[0] in ('§', 'ID', 'Contract', 'Node'):
            continue                      # repeated table headers are not rows
        out.append(cells)
    return out

def payload_files(root):
    out = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d != '__pycache__']
        for fn in filenames:
            if fn == '.DS_Store':
                continue
            out.append(os.path.relpath(os.path.join(dirpath, fn), root))
    return sorted(out)


# ---------------------------------------------------------------- structure
def check_build_dag(root, res, d):
    txt = read(root, 'BUILD_IMPLEMENTATION_DAG.md')
    deps, wave = {}, {}
    for node, dep, w in table_rows(txt, 3):
        n = node.strip('`')
        if not n.startswith('BUILD-A2-'):
            continue
        deps[n] = [x.strip().strip('`') for x in dep.split('·') if x.strip() not in ('—', '')]
        deps[n] = [x for x in deps[n] for x in re.findall(r'BUILD-A2-[A-Z\-]+', x)]
        wave[n] = w.strip()
    d['BUILD_A2_COUNT'] = len(deps)
    d['BUILD_DAG_NODES'] = len(deps)
    d['BUILD_DAG_EDGES'] = sum(len(v) for v in deps.values())
    if len(deps) != 7:
        res['BUILD_A2_COUNT_MISMATCH'] += 1
    # cycles
    colour = {}
    def visit(n):
        colour[n] = 1
        for m in deps.get(n, []):
            if colour.get(m) == 1:
                res['BUILD_DAG_CYCLES'] += 1
            elif colour.get(m) is None:
                visit(m)
        colour[n] = 2
    for n in deps:
        if colour.get(n) is None:
            visit(n)
    # a node's wave must be strictly later than every dependency's wave
    for n, ds in deps.items():
        for m in ds:
            if m in wave and wave[m] >= wave[n]:
                res['WAVE_ORDER_VIOLATIONS'] += 1
    # charters on disk must equal DAG nodes
    charters = [f for f in os.listdir(os.path.join(root, 'BUILD_A2_MANAGERS')) if f.endswith('.md')]
    d['BUILD_A2_MANAGER_FILES'] = len(charters)
    if len(charters) != 7:
        res['BUILD_A2_COUNT_MISMATCH'] += 1
    return deps, wave


def load_schemas(root):
    sd = os.path.join(root, 'schemas')
    out = {}
    for fn in sorted(os.listdir(sd)):
        if fn.endswith('.json'):
            with open(os.path.join(sd, fn), encoding='utf-8') as f:
                out[fn] = json.load(f)
    return out


def validator_for(schemas, schema):
    """jsonschema validator with local $id resolution; unavailable is a hard gate."""
    if not SCHEMA_VALIDATOR_AVAILABLE:
        return None
    reg = Registry()
    for s in schemas.values():
        if '$id' in s:
            reg = reg.with_resource(s['$id'], Resource.from_contents(s))
    return Draft202012Validator(schema, registry=reg)


def check_schemas(root, res, d, owners):
    schemas = load_schemas(root)
    d['SCHEMA_COUNT'] = len(schemas)
    for fn, s in schemas.items():
        if not all(k in s for k in ('$schema', '$id', 'title', 'x-owner')):
            res['SCHEMA_INVALID'] += 1
            continue
        try:
            if SCHEMA_VALIDATOR_AVAILABLE:
                Draft202012Validator.check_schema(s)
        except Exception:
            res['SCHEMA_INVALID'] += 1
            continue
        if s['x-owner'] not in MANAGERS:
            res['SCHEMA_OWNER_MISMATCH'] += 1
            continue
        name = fn[:-len('.schema.json')]
        if name in owners and owners[name] != s['x-owner']:
            res['SCHEMA_OWNER_MISMATCH'] += 1
        # no vendor/model brand may be frozen into an enum (I-17)
        for m in re.finditer(r'"enum"\s*:\s*\[(.*?)\]', json.dumps(s), re.S):
            if re.search(r'gpt-|claude-3|claude-4|gemini|llama-', m.group(1), re.I):
                res['VENDOR_ENUM_VIOLATIONS'] += 1
    return schemas


def check_graph_fixtures(root, res, d, schemas):
    pos = sorted(f for f in os.listdir(os.path.join(root, 'fixtures', 'graphs')) if f.endswith('.json'))
    neg = sorted(f for f in os.listdir(os.path.join(root, 'fixtures', 'graphs-negative')) if f.endswith('.json'))
    d['GRAPH_FIXTURE_COUNT'] = len(pos)
    d['GRAPH_NEGATIVE_FIXTURE_COUNT'] = len(neg)
    gv = validator_for(schemas, schemas['ExecutionGraph.schema.json'])
    ev = validator_for(schemas, schemas['GraphEdge.schema.json'])
    for fn in pos:
        with open(os.path.join(root, 'fixtures', 'graphs', fn), encoding='utf-8') as f:
            g = json.load(f)
        if gv is not None and list(gv.iter_errors(g)):
            res['GRAPH_SCHEMA_FAILURES'] += 1
        # precedence sub-graph must be acyclic
        adj = defaultdict(list)
        for e in g.get('edges', []):
            if e.get('edge_class') == 'PRECEDENCE':
                adj[e['from_node']].append(e['to_node'])
        colour = {}
        def visit(n):
            colour[n] = 1
            for m in adj.get(n, []):
                if colour.get(m) == 1:
                    res['PRECEDENCE_DAG_CYCLE_COUNT'] += 1
                elif colour.get(m) is None:
                    visit(m)
            colour[n] = 2
        for n in list(adj):
            if colour.get(n) is None:
                visit(n)
    for fn in neg:
        with open(os.path.join(root, 'fixtures', 'graphs-negative', fn), encoding='utf-8') as f:
            e = json.load(f)
        rejected = False
        if ev is not None:
            rejected = bool(list(ev.iter_errors(e)))
        else:  # explicit fallback: the exclusivity rule, hand-checked
            ec = e.get('edge_class')
            rejected = ((ec == 'PRECEDENCE' and ('control_kind' in e or 'precedence_kind' not in e)) or
                        (ec == 'CONTROL' and ('precedence_kind' in e or 'control_kind' not in e)))
        if not rejected:
            res['INVALID_GRAPH_EDGE_CLASS_COMBINATIONS_ACCEPTED'] += 1


def check_admission_fixtures(root, res, d, schemas):
    ad = os.path.join(root, 'fixtures', 'admission')
    files = sorted(f for f in os.listdir(ad) if f.endswith('.json'))
    d['ADMISSION_FIXTURE_COUNT'] = len(files)
    # most specific discriminator first: an admission decision may also carry
    # activation_state, so keying on that alone picks the wrong schema
    pick = [('axis_results', 'DispatchAdmissionDecision.schema.json'),
            ('dispatch_permitted', 'FeatureAdmissionDecision.schema.json'),
            ('activation_state', 'ActivationState.schema.json')]
    for fn in files:
        with open(os.path.join(ad, fn), encoding='utf-8') as f:
            obj = json.load(f)
        schema_name = None
        for key, sn in pick:
            if key in obj:
                schema_name = sn
                break
        if schema_name is None:
            res['ADMISSION_FIXTURE_FAILURES'] += 1
            continue
        v = validator_for(schemas, schemas[schema_name])
        if v is not None and list(v.iter_errors(obj)):
            res['ADMISSION_FIXTURE_FAILURES'] += 1
    # axis separation: the entitlement decision may not carry other axes
    fa = json.dumps(schemas['FeatureAdmissionDecision.schema.json'])
    for pat, gate in ((r'provider_(policy|auth|availability)', 'FEATURE_ADMISSION_PROVIDER_OUTCOMES'),
                      (r'safety_(state|interruption)', 'FEATURE_ADMISSION_SAFETY_OUTCOMES')):
        res[gate] += len(re.findall(pat, fa))


def check_ownership(root, res, d):
    ccg = read(root, 'CONTRACT_CONSUMPTION_GRAPH.md')
    owners, seen = {}, Counter()
    for name, owner in table_rows(ccg, 2):
        n, o = name.strip('`'), owner.strip('`*')
        if o in MANAGERS:
            seen[n] += 1
            owners[n] = o
    res['CONTRACT_OWNER_COLLISIONS'] += sum(1 for n, c in seen.items() if c > 1)
    d['INDIVIDUAL_CONTRACTS'] = len(owners)

    # path prefixes resolve to exactly one owner
    om = read(root, 'BUILD_A2_OWNERSHIP_MATRIX.md')
    path_owner = {}
    for cells in table_rows(om, 2):
        paths, owner = cells
        o = owner.strip('`*')
        if o not in MANAGERS:
            continue
        for p in re.findall(r'`([^`]+)`', paths):
            if path_owner.get(p, o) != o:
                res['PATH_OWNER_COLLISIONS'] += 1
            path_owner[p] = o
    d['OWNED_PATH_PREFIXES'] = len(path_owner)

    # each charter's NORMATIVE owned-contract list == the canonical map, both ways
    md = os.path.join(root, 'BUILD_A2_MANAGERS')
    for fn in sorted(os.listdir(md)):
        if not fn.endswith('.md'):
            continue
        txt = open(os.path.join(md, fn), encoding='utf-8').read()
        mid = re.search(r'`(BUILD-A2-[A-Z\-]+)`', txt)
        if not mid:
            res['MANAGER_OWNED_CONTRACT_SET_MISMATCHES'] += 1
            continue
        mgr = mid.group(1)
        sec = re.search(r'## Owned contracts(.*?)(?=\n## )', txt, re.S)
        if not sec:
            res['MANAGER_OWNED_CONTRACT_SET_MISMATCHES'] += 1
            continue
        body = sec.group(1)
        normative = body.split('### [HISTORICAL]')[0]
        if body.count('NORMATIVE — generated from the canonical ownership map') != 1:
            res['MANAGER_DUPLICATE_NORMATIVE_OWNERSHIP_SECTIONS'] += 1
        listed = set(re.findall(r'`([A-Za-z0-9]+)`', normative))
        canonical = {c for c, o in owners.items() if o == mgr}
        if listed != canonical:
            res['MANAGER_OWNED_CONTRACT_SET_MISMATCHES'] += 1
        # a manager never consumes what it owns
        csec = re.search(r'## Consumed contracts(.*?)(?=\n## )', txt, re.S)
        if csec:
            consumed = set(re.findall(r'`([A-Za-z0-9]+)`', csec.group(1)))
            res['SELF_OWNED_EXTERNAL_CONSUMPTIONS'] += len(consumed & canonical)
    return owners


def check_ownership_rule(root, res, d, owners):
    """The generic rule must not contradict the declared runtime flow.

    A sentence of the form "<class> contracts are owned by the producer" is a
    universal claim. It is false the moment RUNTIME_INTERACTION_GRAPH declares a
    contract whose producer is not its owner — which is exactly the case for
    ModelObservation (produced by REVIEW-INTEGRATION, owned by MODEL-ROUTING).
    """
    ccg = read(root, 'CONTRACT_CONSUMPTION_GRAPH.md')
    rig = read(root, 'RUNTIME_INTERACTION_GRAPH.md')
    def mgr(x):
        x = x.strip('`* ')
        return x if x.startswith('BUILD-A2-') else 'BUILD-A2-' + x
    flow = {}
    for cells in table_rows(rig, 4):
        name = cells[0].strip('`')
        m = re.search(r'([A-Z\-]+)\s*(?:→|->)\s*([A-Z\-]+)', cells[1])
        if name and m and cells[2].strip():
            flow[name] = (mgr(m.group(1)), mgr(m.group(2)), mgr(cells[2]))
    d['RUNTIME_FLOW_ROWS'] = len(flow)
    # Every UNIVERSAL producer-ownership claim is checked against the flow table.
    # A line that names the generalisation in order to reject it is not an
    # assertion of it — quoting a falsehood to correct it must not be flagged.
    NEGATED = ('false', 'never', 'not a universal', 'must not', 'contradict',
               '[HISTORICAL]', 'pattern')
    for line in ccg.splitlines():
        if 'owned by the producer' not in line:
            continue
        if any(w in line.lower() for w in NEGATED):
            continue
        for name, (prod, cons, owner) in flow.items():
            if name in line and owner != prod:
                res['CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS'] += 1
        # an unqualified universal rule is itself a contradiction when ANY
        # declared flow row has owner != producer
        if not any(name in line for name in flow):
            if any(owner != prod for _n, (prod, _c, owner) in flow.items()):
                res['CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS'] += 1
    # and the invariant must actually be stated
    if 'NOT DATA-FLOW DIRECTION' not in ccg.upper():
        res['CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS'] += 1
    # the concrete owners in both documents must agree
    for name, (prod, cons, owner) in flow.items():
        if name in owners and mgr(owners[name]) != owner:
            res['CONTRACT_OWNERSHIP_RULE_RUNTIME_FLOW_CONTRADICTIONS'] += 1


def check_wave_contract_dependencies(root, res, d, deps, wave):
    """Each charter's prose HARD_BUILD_DEPENDENCIES must match the DAG, and match wave order.

    NOTE: `## Consumed contracts` / `FROZEN_CONTRACT_DEPENDENCIES` sections are
    intentionally NOT checked here. Those name contracts frozen at M0 before
    any wave begins (`IMPLEMENTATION_MILESTONES.md`), so a same- or
    later-wave *contract* reference is by design, not a defect — S19 exists
    specifically to prove that pattern safe (`ORCHESTRATION` consumes
    `A4Review`, owned by the later-wave `REVIEW-INTEGRATION`). What must
    still hold is narrower: a *concrete build* dependency (HARD_BUILD_DEPENDENCIES,
    the thing WAVE_ORDER_VIOLATIONS checks from the DAG table) must be the
    same set a manager's own charter prose declares, and must resolve to a
    strictly earlier wave — a mismatch here means the DAG and the charter
    prose disagree about what this manager actually needs built first.
    """
    md = os.path.join(root, 'BUILD_A2_MANAGERS')
    n = 0
    for fn in sorted(os.listdir(md)):
        if not fn.endswith('.md'):
            continue
        txt = open(os.path.join(md, fn), encoding='utf-8').read()
        mid = re.search(r'`(BUILD-A2-[A-Z\-]+)`', txt)
        if not mid:
            continue
        mgr = mid.group(1)
        if mgr not in wave:
            continue
        hsec = re.search(r'## HARD_BUILD_DEPENDENCIES(.*?)(?=\n## )', txt, re.S)
        prose_deps = set(re.findall(r'-\s*`(BUILD-A2-[A-Z\-]+)`', hsec.group(1))) if hsec else set()
        dag_deps = set(deps.get(mgr, []))
        n += len(prose_deps ^ dag_deps)     # charter prose and DAG table must agree
        for dep in prose_deps | dag_deps:
            if dep in wave and wave[dep] >= wave[mgr]:
                n += 1
    res['UNSATISFIED_SAME_OR_LATER_WAVE_CONTRACT_DEPENDENCIES'] += n


def check_required_paths(root, res, d):
    """Path-ownership COMPLETENESS, not just collision-freedom (BUILD-A1 §3).

    `REPOSITORY_LAYOUT_PROPOSAL.md`'s "Required path authority" table is the
    canonical, machine-readable list of paths this architecture requires.
    `BUILD_A2_OWNERSHIP_MATRIX.md`'s "Source ownership" and
    "BUILD-A1-controlled directories" tables are the canonical resolution.
    Neither is prose-grepped: both are markdown tables parsed the same way
    every other structural check in this module parses one.
    """
    valid_owners = set(MANAGERS) | {'BUILD-A1'}

    def owner_table(text):
        out = {}
        for cells in table_rows(text, 2):
            paths, owner_cell = cells
            o = owner_cell.strip('`* ')
            if o not in valid_owners:
                continue
            for p in re.findall(r'`([^`]+)`', paths):
                out[p] = o
        return out

    om = read(root, 'BUILD_A2_OWNERSHIP_MATRIX.md')
    resolved = owner_table(om)

    rl = read(root, 'REPOSITORY_LAYOUT_PROPOSAL.md')
    sec = re.search(r'## Required path authority(.*?)(?=\n## )', rl, re.S)
    required = {}
    if sec:
        for path, owner_cell in table_rows(sec.group(1), 2):
            p = path.strip('`')
            if p == 'Required path':
                continue                  # the table's own header row
            required[p] = owner_cell.strip('`* ')
    d['REQUIRED_PATH_COUNT'] = len(required)

    for path, want_owner in required.items():
        if path not in resolved:
            res['UNOWNED_REQUIRED_PATHS'] += 1
            continue
        got_owner = resolved[path]
        if got_owner != want_owner:
            res['REQUIRED_PATH_OWNER_MISMATCHES'] += 1
    # ambiguity: the same required path must not be assigned to two owners
    # by two different rows of the ownership matrix
    seen = Counter()
    for cells in table_rows(om, 2):
        paths, owner_cell = cells
        o = owner_cell.strip('`* ')
        if o not in valid_owners:
            continue
        for p in re.findall(r'`([^`]+)`', paths):
            if p in required:
                seen[p] += 1
    for path, count in seen.items():
        if count > 1:
            res['AMBIGUOUS_REQUIRED_PATHS'] += 1


def check_m0_classification(root, res, d, schemas):
    """M0's machine/behavioural split must match the actual schemas/ directory.

    V1.3.3 shipped `schemas/HostCapabilityReport.schema.json` on disk while
    `IMPLEMENTATION_MILESTONES.md` classified `HostCapabilityReport` as
    non-schema/behavioural — a direct contradiction between the package's own
    contents and its own milestone document. The machine-schema set below is
    derived from `schemas/` (via `schemas`, already loaded), never asserted.
    """
    txt = read(root, 'IMPLEMENTATION_MILESTONES.md')
    actual_schema_names = {fn[:-len('.schema.json')] for fn in schemas}
    d['M0_SCHEMA_COUNT_ACTUAL'] = len(actual_schema_names)

    msec = re.search(r'### Machine-validatable M0 schemas \((\d+)\)\s*\n\n(.*?)\n\n', txt, re.S)
    doc_schema_names, doc_schema_count = set(), None
    if msec:
        doc_schema_count = int(msec.group(1))
        doc_schema_names = set(re.findall(r'`([A-Za-z0-9]+)`', msec.group(2)))
    else:
        res['M0_SCHEMA_CLASSIFICATION_MISMATCHES'] += 1

    bsec = re.search(r'### Non-schema / behavioural M0 contracts \((\d+)\)(.*?)(?=\n### |\n## )', txt, re.S)
    doc_behavioural_names, doc_behavioural_count = set(), None
    if bsec:
        doc_behavioural_count = int(bsec.group(1))
        doc_behavioural_names = {r[0].strip('`') for r in table_rows(bsec.group(2), 3)}
    else:
        res['M0_SCHEMA_CLASSIFICATION_MISMATCHES'] += 1
    d['M0_BEHAVIOURAL_COUNT_ACTUAL'] = len(doc_behavioural_names)

    # a schema on disk classified as behavioural, or vice versa, is the exact
    # contradiction this gate exists to catch
    res['M0_SCHEMA_CLASSIFICATION_MISMATCHES'] += len(doc_behavioural_names & actual_schema_names)
    res['M0_SCHEMA_CLASSIFICATION_MISMATCHES'] += len(doc_schema_names - actual_schema_names)
    res['M0_SCHEMA_CLASSIFICATION_MISMATCHES'] += len(actual_schema_names - doc_schema_names)
    if doc_schema_count is not None and doc_schema_count != len(doc_schema_names):
        res['M0_SCHEMA_CLASSIFICATION_MISMATCHES'] += 1
    if doc_behavioural_count is not None and doc_behavioural_count != len(doc_behavioural_names):
        res['M0_SCHEMA_CLASSIFICATION_MISMATCHES'] += 1

    d['M0_SCHEMA_COUNT_MATCHES_ACTUAL'] = 'YES' if (
        doc_schema_names == actual_schema_names and doc_schema_count == len(actual_schema_names)) else 'NO'
    d['M0_BEHAVIOURAL_CONTRACT_COUNT_MATCHES_ACTUAL'] = 'YES' if (
        doc_behavioural_count == len(doc_behavioural_names) and
        not (doc_behavioural_names & actual_schema_names)) else 'NO'


# V1.3.6: each negative fixture is tagged with the specific defect category it
# proves is rejected, so the general HOST_CAPABILITY_INVALID_STATE_ACCEPTED
# gate can be broken out into the three named gates BUILD-A1 required. A
# fixture not listed here only feeds the general gate.
NEGATIVE_FIXTURE_CATEGORY_GATE = {
    '02_plugin_installed_without_support.json': 'PLUGIN_INSTALLED_WITHOUT_SUPPORT_ACCEPTED',
    '01_embedded_with_untrusted_hooks.json': 'HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED',
    '03_embedded_with_stale_inactive_reason.json': 'HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED',
    '04_supervised_falsely_claims_no_inactive_reason.json': 'HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED',
    '05_trust_model_ambiguity_codex_null.json': 'HOST_CAPABILITY_TRUST_MODEL_AMBIGUITY_ACCEPTED',
    '06_inactive_reason_hooks_disabled_mismatch.json': 'HOST_CAPABILITY_INACTIVE_REASON_MISMATCHES_ACCEPTED',
    '07_insufficient_coverage_embedded.json': 'HOST_CAPABILITY_INSUFFICIENT_COVERAGE_EMBEDDED_ACCEPTED',
    '08_stale_embedded.json': 'STALE_HOST_CAPABILITY_REPORT_EMBEDDED_ACCEPTED',
    '09_known_plugin_missing_unknown_reason.json': 'UNKNOWN_REASON_WITH_KNOWN_FAILURE_ACCEPTED',
    '10_healthy_supervised_without_override.json': 'HEALTHY_NATIVE_PATH_FALLBACK_WITHOUT_OVERRIDE_ACCEPTED',
    '11_hooks_configured_without_support.json': 'HOOKS_CONFIGURED_WITHOUT_SUPPORT_ACCEPTED',
    '12_hooks_enabled_without_support.json': 'HOOKS_ENABLED_WITHOUT_SUPPORT_ACCEPTED',
    '13_hook_trust_required_unknown.json': 'HOOK_TRUST_REQUIRED_UNKNOWN_ACCEPTED',
}


def check_host_capability_fixtures(root, res, d, schemas):
    hcs = schemas.get('HostCapabilityReport.schema.json')
    v = validator_for(schemas, hcs) if hcs else None
    pos = sorted(f for f in os.listdir(os.path.join(root, 'fixtures', 'host_capability')) if f.endswith('.json'))
    neg = sorted(f for f in os.listdir(os.path.join(root, 'fixtures', 'host_capability-negative')) if f.endswith('.json'))
    d['HOST_CAPABILITY_FIXTURE_COUNT'] = len(pos)
    d['HOST_CAPABILITY_NEGATIVE_FIXTURE_COUNT'] = len(neg)
    for fn in pos:
        obj = json.load(open(os.path.join(root, 'fixtures', 'host_capability', fn), encoding='utf-8'))
        if (v is not None and list(v.iter_errors(obj))) or not valid_host_report(obj):
            res['HOST_CAPABILITY_VALID_STATE_REJECTED'] += 1
    for fn in neg:
        obj = json.load(open(os.path.join(root, 'fixtures', 'host_capability-negative', fn), encoding='utf-8'))
        if v is not None and not list(v.iter_errors(obj)) and valid_host_report(obj):
            res['HOST_CAPABILITY_INVALID_STATE_ACCEPTED'] += 1
            gate = NEGATIVE_FIXTURE_CATEGORY_GATE.get(fn)
            if gate:
                res[gate] += 1


LOAD_BEARING = ('plugin_installed', 'hooks_supported', 'hooks_configured',
                'hooks_enabled',
                'hooks_allowed_by_admin_policy', 'required_hook_coverage_satisfied')
COMPLETE_REQUIRED = LOAD_BEARING + ('plugin_supported', 'hook_trust_required')

def valid_host_report(x):
    """Semantic rules JSON Schema cannot express compactly; false is known failure, null unknown."""
    complete = x.get('probe_status') == 'COMPLETE'
    fresh = x.get('stale_reason') == 'NONE' and bool(x.get('validity_fingerprint'))
    healthy = all(x.get(k) is True for k in LOAD_BEARING) and (
        x.get('hook_trust_required') is False or x.get('hooks_trusted') is True)
    if complete and any(x.get(k) is None for k in COMPLETE_REQUIRED): return False
    if complete and x.get('plugin_installed') is True and x.get('plugin_supported') is not True: return False
    if complete and x.get('hooks_configured') is True and x.get('hooks_supported') is not True: return False
    if complete and x.get('hooks_enabled') is True and x.get('hooks_supported') is not True: return False
    if complete and x.get('hook_trust_required') is True and x.get('hooks_trusted') not in (True, False): return False
    if x.get('probe_status') == 'FAILED' and x.get('stale_reason') != 'PROBE_FAILED': return False
    if x.get('selected_mode') == 'EMBEDDED':
        if not (complete and fresh and healthy and x.get('inactive_reason') == 'NONE'): return False
    if x.get('selected_mode') != 'EMBEDDED' and x.get('inactive_reason') == 'NONE': return False
    reason = x.get('inactive_reason')
    rungs = [('PLUGIN_NOT_INSTALLED','plugin_installed',False), ('HOOKS_UNSUPPORTED','hooks_supported',False),
             ('HOOKS_NOT_CONFIGURED','hooks_configured',False), ('HOOKS_UNTRUSTED','hooks_trusted',False),
             ('HOOKS_DISABLED','hooks_enabled',False), ('HOOKS_EXCLUDED_BY_ADMIN_POLICY','hooks_allowed_by_admin_policy',False),
             ('INSUFFICIENT_COVERAGE','required_hook_coverage_satisfied',False)]
    known = [name for name,key,val in rungs if x.get(key) is val]
    if reason == 'UNKNOWN' and known: return False
    if known and reason != known[0]: return False
    if complete and healthy and x.get('selected_mode') != 'EMBEDDED':
        if not x.get('mode_override') or reason != 'MODE_OVERRIDE': return False
    if reason == 'UNKNOWN' and complete: return False
    return True


def check_freshness_and_event_authority(root, res, d, schemas):
    freshness = json.load(open(os.path.join(root, 'evidence', 'HOST_CAPABILITY_FRESHNESS_AUTHORITY.json'), encoding='utf-8'))
    required_triggers = {'INITIAL_INSTALL','PLUGIN_INSTALL_OR_REMOVAL','PLUGIN_UPDATE','HOOK_DEFINITION_CHANGE','HOST_VERSION_CHANGE','TRUST_STATE_CHANGE','HOOK_ENABLEMENT_OR_CONFIG_CHANGE','ADMIN_POLICY_CHANGE','SESSION_START_OR_RESUME','UNPROVEN_VALIDITY_BEFORE_EMBEDDED'}
    if set(freshness.get('reprobe_triggers', [])) != required_triggers: res['HOST_CAPABILITY_FRESHNESS_TRIGGER_MISMATCHES'] += 1
    for rel in ('HOST_CAPABILITY_DISCOVERY.md','HOST_ARCHITECTURE.md','CODEX_HOST_ADAPTER.md','CLAUDE_HOST_ADAPTER.md','CROSS_HOST_RESUME.md','PLUGIN_DISTRIBUTION_ARCHITECTURE.md','CODEX_PLUGIN_PACKAGING.md','BUILD_A2_MANAGERS/BUILD_A2_HOST_INTEGRATION.md','SCENARIO_VALIDATION.md'):
        if 'HOST_CAPABILITY_FRESHNESS_AUTHORITY.json' not in read(root, rel): res['HOST_CAPABILITY_FRESHNESS_TRIGGER_MISMATCHES'] += 1
    auth = json.load(open(os.path.join(root, 'evidence', 'HOST_EVENT_SOURCE_AUTHORITY.json'), encoding='utf-8'))
    authority_schema = json.load(open(os.path.join(root, 'evidence', 'HOST_EVENT_SOURCE_AUTHORITY.schema.json'), encoding='utf-8'))
    av = validator_for({}, authority_schema)
    if av is not None and list(av.iter_errors(auth)): res['EVENT_AUTHORITY_SCHEMA_INVALID'] += 1
    allowed = set(auth.get('source_classes', [])); events = auth.get('events', [])
    if not allowed or not isinstance(events, list): res['EVENT_AUTHORITY_SCHEMA_INVALID'] += 1; return
    names = [e.get('event') for e in events]
    res['EVENT_AUTHORITY_DUPLICATE_EVENTS'] += len(names) - len(set(names))
    for e in events:
        sources = e.get('sources')
        if not isinstance(sources, dict) or not {'claude-code','codex-worker'} <= set(sources): res['EVENT_AUTHORITY_MISSING_REQUIRED_HOST_SOURCES'] += 1; continue
        for s in sources.values():
            if not isinstance(s, dict) or not isinstance(s.get('source'), str) or s.get('source_class') not in allowed: res['EVENT_AUTHORITY_UNKNOWN_SOURCE_CLASSES'] += 1
    table = {r[0].strip('`'): r for r in table_rows(read(root, 'NORMALIZED_HOST_EVENTS.md'), 5)}
    for e in events:
        row = table.get(e.get('event'))
        if not row: res['NORMALIZED_HOST_EVENT_TABLE_AUTHORITY_MISMATCHES'] += 1; continue
        cs, ds = e['sources']['claude-code'], e['sources']['codex-worker']
        source_class = cs['source_class'] if cs['source_class'] == ds['source_class'] else cs['source_class'] + ' / ' + ds['source_class']
        if row[2] != source_class or row[3].replace('`', '') != cs['source'] or row[4].replace('`', '') != ds['source']:
            res['NORMALIZED_HOST_EVENT_TABLE_AUTHORITY_MISMATCHES'] += 1


def check_host_capability_doc_fields(root, res, d, schemas):
    """One canonical HostCapabilityReport field vocabulary (BUILD-A1 §5)."""
    hcs = schemas.get('HostCapabilityReport.schema.json')
    schema_fields = set(hcs.get('properties', {})) if hcs else set()
    STALE = {'supports_plugin_manifest', 'supports_hooks', 'plugin_hooks_trusted', 'hooks_feature_enabled'}
    txt = read(root, 'HOST_CAPABILITY_DISCOVERY.md')
    field_line = re.search(r'`host_id`.*', txt)
    doc_fields = set()
    if field_line:
        doc_fields = {f.strip('`* ') for f in field_line.group(0).split('\xb7')}
        doc_fields = {f[:-2] if f.endswith('[]') else f for f in doc_fields}
    res['HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES'] += len(doc_fields - schema_fields)
    for stale in STALE:
        if re.search(r'`' + re.escape(stale) + r'`', txt):
            res['HOST_CAPABILITY_DOC_SCHEMA_FIELD_MISMATCHES'] += 1


def check_parity(root, res, d):
    txt = read(root, 'HOST_PARITY_CONTRACT.md')
    rows = [r for r in table_rows(txt, 3) if re.match(r'P-\d+$', r[0].strip('* '))]
    count = len(rows)
    d['PARITY_CAPABILITY_COUNT'] = count

    for rel in ['IMPLEMENTATION_MILESTONES.md', 'HOST_PARITY_CONTRACT.md']:
        t = read(root, rel)
        for m in re.finditer(r'(\d+)\s+parity rows', t):
            if int(m.group(1)) != count:
                res['PARITY_DISPLAYED_COUNT_MISMATCHES'] += 1

    scaffold = re.search(r'p01_\w+\.spec\s*…\s*p(\d+)_\w+\.spec', txt)
    if not scaffold or int(scaffold.group(1)) != count:
        res['PARITY_CONFORMANCE_COVERAGE_MISSING'] += 1


# ---------------------------------------------------------------- documents
def check_open_questions(root, res, d):
    txt = read(root, 'OPEN_QUESTIONS.md')
    reg = json.load(open(os.path.join(root, 'evidence', 'SOURCE_CLAIM_REGISTRY.json'), encoding='utf-8'))
    label = {c['id']: c['label'] for c in reg['claims']}
    rows = [r for r in table_rows(txt, 7) if r[0].strip('* ').startswith('Q-')]
    counts, status = Counter(), {}
    for rid, n, _q, _own, cls, ev, _disp in rows:
        qid = rid.strip('* ')
        cl = cls.strip('*` ')
        counts[cl] += int(n)
        status[qid] = cl
        cited = re.findall(r'`(C-[A-Z0-9a-z\-]+)`', ev)
        cited = [c for c in cited if c in label]
        if cl == 'RESOLVED':
            # every cited claim must be current evidence
            for c in cited:
                if label[c] not in CURRENT_EVIDENCE_LABELS:
                    res['OPEN_QUESTION_STATUS_SOURCE_MISMATCHES'] += 1
        elif cited:
            # an open question must rest on something genuinely not established
            if not any(label[c] in OPEN_EVIDENCE_LABELS for c in cited):
                res['OPEN_QUESTION_STATUS_SOURCE_MISMATCHES'] += 1
    d['OPEN_QUESTION_ROWS'] = len(rows)
    d['OPEN_QUESTION_TOTAL'] = sum(counts.values())
    for k, v in counts.items():
        d['OQ_' + k] = v
    # the displayed summary block must equal the row-derived counts
    block = re.search(r'## Blocking summary.*?```(.*?)```', txt, re.S)
    if not block:
        res['OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES'] += 1
    else:
        shown = {k: int(v) for k, v in re.findall(r'([A-Z_]+)\s*=\s*(\d+)', block.group(1))}
        for k, v in shown.items():
            if k == 'TOTAL':
                if v != sum(counts.values()):
                    res['OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES'] += 1
            elif counts.get(k, 0) != v:
                res['OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES'] += 1
        for k, v in counts.items():
            if k not in shown:
                res['OPEN_QUESTION_BLOCKING_SUMMARY_MISMATCHES'] += 1
    return status


def check_traceability(root, res, d, qstatus):
    txt = read(root, 'REQUIREMENTS_TRACEABILITY_MATRIX.md')
    body, _, hist = txt.partition('## [HISTORICAL] V1.2 traceability snapshot')
    rows = [r for r in table_rows(body, 8) if r[7]]
    counts = Counter('SPECIFIED-PARTIAL' if 'SPECIFIED-PARTIAL' in r[7]
                     else ('SPECIFIED' if 'SPECIFIED' in r[7] else r[7].strip('* '))
                     for r in rows)
    d['TRACE_ROWS'] = len(rows)
    d['TRACE_SPECIFIED'] = counts.get('SPECIFIED', 0)
    d['TRACE_PARTIAL'] = counts.get('SPECIFIED-PARTIAL', 0)
    d['TRACE_DEFERRED'] = counts.get('DEFERRED', 0)
    d['TRACE_BLOCKED'] = counts.get('BLOCKED', 0)

    # no current row may name a question that OPEN_QUESTIONS reports as resolved
    for r in rows:
        for q in re.findall(r'Q-[A-Z0-9\-]+', r[7]):
            if qstatus.get(q) == 'RESOLVED':
                res['TRACEABILITY_STALE_RESOLVED_QUESTION_REFS'] += 1
            elif q not in qstatus:
                res['TRACEABILITY_STALE_RESOLVED_QUESTION_REFS'] += 1

    # exactly one CURRENT coverage summary; the historical one must be marked
    summaries = body.count('| **TOTAL** |')
    if summaries != 1:
        res['TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES'] += abs(summaries - 1)
    if hist and '[HISTORICAL]' not in hist.split('\n\n')[0] + hist[:400]:
        res['TRACEABILITY_DUPLICATE_CURRENT_SUMMARIES'] += 1

    shown = {k: int(v) for k, v in re.findall(
        r'\|\s*`?(SPECIFIED-PARTIAL|SPECIFIED|DEFERRED|BLOCKED)`?\s*\|\s*(\d+)\s*\|', body)}
    total = re.search(r'\|\s*\*\*TOTAL\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|', body)
    if shown.get('SPECIFIED') != d['TRACE_SPECIFIED']:
        res['TRACEABILITY_STATUS_COUNT_MISMATCHES'] += 1
    if shown.get('SPECIFIED-PARTIAL') != d['TRACE_PARTIAL']:
        res['TRACEABILITY_STATUS_COUNT_MISMATCHES'] += 1
    if not total or int(total.group(1)) != len(rows):
        res['TRACEABILITY_CURRENT_ROW_COUNT_MISMATCH'] += 1


def check_authority_counts(root, res, d):
    docs = sorted([f for f in os.listdir(root) if f.endswith('.md')] +
                  [os.path.join('BUILD_A2_MANAGERS', f)
                   for f in os.listdir(os.path.join(root, 'BUILD_A2_MANAGERS')) if f.endswith('.md')])
    cn = hs = 0
    for rel in docs:
        t = read(root, rel)
        m = re.search(r'DOCUMENT_AUTHORITY:\s*(CURRENT_NORMATIVE|HISTORICAL_SNAPSHOT)', t)
        if m and m.group(1) == 'CURRENT_NORMATIVE':
            cn += 1
        elif m:
            hs += 1
    # measured over exactly the set evidence/validate_sources.py scans
    d['CURRENT_NORMATIVE_DOCS'] = cn
    d['HISTORICAL_SNAPSHOT_DOCS'] = hs


def check_scenarios_and_registry(root, res, d):
    sc = read(root, 'SCENARIO_VALIDATION.md')
    d['SCENARIO_COUNT'] = len({s.upper().replace('-', '') for s in re.findall(r'\bS-?\d+\b', sc)})
    reg = json.load(open(os.path.join(root, 'evidence', 'SOURCE_CLAIM_REGISTRY.json'), encoding='utf-8'))
    d['SOURCE_CLAIM_COUNT'] = len(reg['claims'])
    d['POLICY_MATRIX_ROWS'] = len(reg['policy_matrix'])


# ---------------------------------------------------------------- archive
def check_manifests(root, res, d, files):
    for name, key in (('PACKAGE_MANIFEST.sha256', 'PACKAGE_MANIFEST_ENTRY_COUNT'),
                      ('INSTALL_MANIFEST.sha256', 'INSTALL_MANIFEST_ENTRY_COUNT')):
        path = os.path.join(root, name)
        if not os.path.exists(path):
            d[key] = 0
            continue
        entries = [l.split(None, 1) for l in open(path, encoding='utf-8').read().splitlines() if l.strip()]
        d[key] = len(entries)
        for digest, rel in entries:
            rel = rel.strip().lstrip('*')
            fp = os.path.join(root, rel)
            if not os.path.exists(fp):
                res['MANIFEST_HASH_FAILURES'] += 1
                continue
            h = hashlib.sha256(open(fp, 'rb').read()).hexdigest()
            if h != digest:
                res['MANIFEST_HASH_FAILURES'] += 1
        listed = {e[1].strip().lstrip('*') for e in entries}
        expect = set(files) - {name}
        if name == 'INSTALL_MANIFEST.sha256':
            expect -= {'PACKAGE_MANIFEST.sha256'}
        else:
            expect -= {'INSTALL_MANIFEST.sha256'} if 'INSTALL_MANIFEST.sha256' not in listed else set()
        if listed != expect:
            res['MANIFEST_HASH_FAILURES'] += len(listed ^ expect)


def glob_to_regex(glob):
    """`*` matches within one path segment; `**` matches any depth (incl. zero)."""
    tmp = glob.replace('**', '\0DOUBLE\0').replace('*', '\0SINGLE\0')
    tmp = re.escape(tmp)
    tmp = tmp.replace(re.escape('\0DOUBLE\0'), '.*')
    tmp = tmp.replace(re.escape('\0SINGLE\0'), '[^/]*')
    return re.compile('^' + tmp + '$')


def check_installation_map(root, res, d):
    """Every path in INSTALL_MANIFEST.sha256 must resolve to exactly one rule in
    REPOSITORY_INSTALLATION_MAP.md's Mapping table, and that rule's repository
    path must equal the install root plus the package path unchanged (V1.3.6 §6)."""
    manifest_path = os.path.join(root, 'INSTALL_MANIFEST.sha256')
    if not os.path.exists(manifest_path):
        return
    manifest_paths = [l.split(None, 1)[1].strip().lstrip('*')
                       for l in open(manifest_path, encoding='utf-8').read().splitlines() if l.strip()]

    txt = read(root, 'REPOSITORY_INSTALLATION_MAP.md')
    INSTALL_ROOT = 'build-control/orchestrator-architecture/'
    rules = []
    for cells in table_rows(txt, 2):
        pkg_cell, repo_cell = cells
        m = re.search(r'`([^`]+)`', pkg_cell)
        if not m:
            continue
        pkg_glob = m.group(1)
        rm = re.search(r'`([^`]+)`', repo_cell)
        repo_glob = rm.group(1) if rm else None
        rules.append((pkg_glob, repo_glob, glob_to_regex(pkg_glob)))
        if repo_glob is not None and repo_glob != INSTALL_ROOT + pkg_glob:
            res['INSTALLATION_MAP_PATH_MISMATCHES'] += 1

    for p in manifest_paths:
        matches = [rule for rule in rules if rule[1] is not None and rule[2].match(p)]
        if not matches:
            res['INSTALL_MANIFEST_UNMAPPED_PATHS'] += 1
        else:
            destinations = {INSTALL_ROOT + p for _pg, _rg, _re in matches}
            if len(destinations) > 1:
                res['INSTALLATION_MAP_AMBIGUOUS_PATHS'] += 1


def zip_dimensions(zpath, res, d):
    with zipfile.ZipFile(zpath) as z:
        infos = z.infolist()
        names = [i.filename for i in infos]
        d['ZIP_ENTRY_COUNT'] = len(infos)
        d['FILE_COUNT'] = sum(1 for i in infos if not i.filename.endswith('/'))
        d['DIRECTORY_COUNT'] = sum(1 for i in infos if i.filename.endswith('/'))
        for n in names:
            if n.startswith('/') or '..' in n.split('/') or ':' in n:
                res['ZIP_SAFETY_VIOLATIONS'] += 1
        if len(set(names)) != len(names):
            res['ZIP_SAFETY_VIOLATIONS'] += 1
        low = [n.lower() for n in names]
        if len(set(low)) != len(low):
            res['ZIP_SAFETY_VIOLATIONS'] += 1
        for i in infos:
            if i.flag_bits & 0x1:
                res['ZIP_SAFETY_VIOLATIONS'] += 1
            if (i.external_attr >> 16) & 0o170000 == 0o120000:
                res['ZIP_SAFETY_VIOLATIONS'] += 1
        if z.testzip() is not None:
            res['ZIP_SAFETY_VIOLATIONS'] += 1


def fs_dimensions(root, files, d):
    dirs = set()
    for f in files:
        p = os.path.dirname(f)
        while p:
            dirs.add(p)
            p = os.path.dirname(p)
    d['FILE_COUNT'] = len(files)
    d['DIRECTORY_COUNT'] = len(dirs)
    d['ZIP_ENTRY_COUNT'] = len(files) + len(dirs)


# ---------------------------------------------------------------- display check
KEY_RE = None

def check_displayed(root, res, d, details):
    """Every displayed integer that names a derived key must equal that key.

    This is the gate V1.3.2 asserted without implementing. It reads the three
    generated reports back as text — the same bytes a reviewer reads.
    """
    global KEY_RE
    keys = sorted(d, key=len, reverse=True)
    # word-boundary on BOTH sides: BUILD_A2_COUNT must not match inside
    # BUILD_A2_COUNT_MISMATCH, which is a gate name, not a derived count
    KEY_RE = re.compile(r'(?<![A-Za-z0-9_])(?:' +
                        '|'.join(re.escape(k) for k in keys) + r')(?![A-Za-z0-9_])')
    for rel in REPORTS:
        if not os.path.exists(os.path.join(root, rel)):
            continue
        for lineno, line in enumerate(read(root, rel).splitlines(), 1):
            for m in KEY_RE.finditer(line):
                key = m.group(0)
                tail = line[m.end():]
                # the first integer after the key on this line, if any
                num = re.search(r'(?<![\w.])(\d+)(?![\w.])', tail)
                if not num:
                    continue
                # do not misread a following key's value as this key's value
                nxt = KEY_RE.search(tail)
                if nxt and nxt.start() < num.start():
                    continue
                if int(num.group(1)) != d[key]:
                    res['DISPLAYED_DERIVED_COUNT_MISMATCHES'] += 1
                    details.append((rel, lineno, key, int(num.group(1)), d[key]))


def check_report_gate_markers(root, res):
    """Every generated report carries the exact package-gate object it displays."""
    expected = dict(res)
    for rel in REPORTS:
        path = os.path.join(root, rel)
        if not os.path.exists(path):
            res['FINAL_REPORT_PACKAGE_GATE_MISMATCHES'] += 1
            continue
        marker = re.search(r'FINAL_PACKAGE_GATES:\s*(\{[^\n]*\})', read(root, rel))
        if not marker:
            res['FINAL_REPORT_PACKAGE_GATE_MISMATCHES'] += 1
            continue
        try:
            shown = json.loads(marker.group(1))
        except json.JSONDecodeError:
            shown = None
        if shown != expected:
            res['FINAL_REPORT_PACKAGE_GATE_MISMATCHES'] += 1


# ---------------------------------------------------------------- main
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--root', default=os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    ap.add_argument('--zip', dest='zpath', default=None)
    ap.add_argument('--json', dest='out', default=None)
    ap.add_argument('--no-display-check', action='store_true')
    a = ap.parse_args()
    root = os.path.abspath(a.root)

    res = {g: 0 for g in GATES}
    d, details = {}, []
    d['REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE'] = 'YES' if SCHEMA_VALIDATOR_AVAILABLE else 'NO'
    d['SCHEMA_VALIDATION_EXECUTED'] = 'YES' if SCHEMA_VALIDATOR_AVAILABLE else 'NO'
    if not SCHEMA_VALIDATOR_AVAILABLE:
        res['MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED'] = 1
        res['VALIDATION_DEPENDENCY_FAIL_OPEN'] = 1

    files = payload_files(root)
    deps, wave = check_build_dag(root, res, d)
    owners = check_ownership(root, res, d)
    schemas = check_schemas(root, res, d, owners)
    check_graph_fixtures(root, res, d, schemas)
    check_admission_fixtures(root, res, d, schemas)
    check_ownership_rule(root, res, d, owners)
    check_wave_contract_dependencies(root, res, d, deps, wave)
    check_required_paths(root, res, d)
    check_m0_classification(root, res, d, schemas)
    check_host_capability_fixtures(root, res, d, schemas)
    check_host_capability_doc_fields(root, res, d, schemas)
    check_freshness_and_event_authority(root, res, d, schemas)
    check_parity(root, res, d)
    qstatus = check_open_questions(root, res, d)
    check_traceability(root, res, d, qstatus)
    check_authority_counts(root, res, d)
    check_scenarios_and_registry(root, res, d)
    check_manifests(root, res, d, files)
    check_installation_map(root, res, d)

    if a.zpath:
        zip_dimensions(a.zpath, res, d)          # authoritative: the shipped ZIP
    else:
        fs_dimensions(root, files, d)

    if not a.no_display_check:
        check_displayed(root, res, d, details)
    check_report_gate_markers(root, res)

    for k in sorted(d):
        print(f"{k} = {d[k]}")
    print()
    for g in GATES:
        print(f"{g} = {res[g]}")
    for t in details[:40]:
        print("   MISMATCH", t)

    if a.out:
        with open(a.out, 'w', encoding='utf-8') as f:
            json.dump({'derived': d, 'gates': res, 'details': details,
                       'source': a.zpath or root}, f, indent=1)

    failed = [g for g in GATES if res[g] != 0]
    if failed:
        print(f"\nPACKAGE VALIDATION FAILED: {', '.join(failed)}", file=sys.stderr)
        return 1
    print("\nALL PACKAGE GATES PASS")
    return 0


if __name__ == '__main__':
    sys.exit(main())
