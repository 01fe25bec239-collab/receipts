#!/usr/bin/env python3
"""Regression suite for the source validator.

F1-F6 prove the V1.3.2 validator catches what V1.3.1 missed.
F7-F9 prove the V1.3.3 validator catches DEFECT D and DEFECT E — the header-comment bypass,
where a metadata comment NOT followed by a blank line hid the live prose after
'-->' from every check. F8 is its counterpart: a metadata comment directly
followed by ACCURATE current prose must still pass, so the fix cannot be a
blanket "flag anything after a comment".

Each fixture is scanned in isolation. Both the exit code AND the named gate
value are asserted, so a fixture cannot pass for the wrong reason."""
import subprocess,sys,os,tempfile,shutil,json
HERE=os.path.dirname(os.path.abspath(__file__))
ROOT=os.path.dirname(HERE)
FIX=os.path.join(HERE,'regression')
EXPECT={
 'F1_stale_a14_after_history.md':('FAIL','STALE_A14_CURRENT_ASSERTIONS',
   'DEFECT A: historical marker in a NEIGHBOURING block must not excuse a current stale assertion'),
 'F2_stale_user_declared.md':('FAIL','STALE_USER_DECLARED_CODEX_PLUGIN_ASSERTIONS',
   'C-01/C-02 are verified current; USER_DECLARED is stale'),
 'F3_stale_research_unavailable.md':('FAIL','STALE_RESEARCH_UNAVAILABLE_ASSERTIONS',
   'research closed at V1.3.1'),
 'F4_historical_snapshot_ok.md':('PASS',None,
   'HISTORICAL_SNAPSHOT authority contributes no current assertion'),
 'F5_correct_current.md':('PASS',None,'accurate current statement'),
 'F6_undeclared_authority.md':('FAIL','UNDECLARED_DOCUMENT_AUTHORITY',
   'every source-bearing document must declare authority'),
 'F7_header_comment_without_blank_line.md':('FAIL','STALE_A14_CURRENT_ASSERTIONS',
   'DEFECT D: prose after a metadata comment with no blank line must still be scanned'),
 'F8_header_comment_valid_current.md':('PASS',None,
   'DEFECT D counterpart: comment directly followed by accurate current prose still passes'),
 'F9_table_row_scope_leak.md':('FAIL','STALE_A14_CURRENT_ASSERTIONS',
   'DEFECT E: one table row marked historical must not exempt its sibling rows'),
 'F10_stale_research_could_not_be_performed.md':('FAIL','STALE_RESEARCH_UNAVAILABLE_ASSERTIONS',
   'V1.3.4: "research ... could not be performed" (research-first phrasing) must be caught, not just "could not perform ... research"'),
 'HOST_PARITY_CONTRACT.md':('FAIL','CURRENT_HOST_PRIMARY_MECHANISM_MISMATCHES',
   'V1.3.6 F11: "Shallower; supervisor-mediated" presented as the ordinary Codex in-session path must fail — native plugin/hooks is the canonical primary posture'),
 'CODEX_HOST_ADAPTER.md':('PASS',None,
   'V1.3.6 F12 counterpart: "shallower"/"supervisor-mediated" scoped explicitly to the SUPERVISED fallback must still pass'),
}
def run_one(fixture):
    tmp=tempfile.mkdtemp()
    try:
        shutil.copytree(HERE,os.path.join(tmp,'evidence'),
                        ignore=shutil.ignore_patterns('regression','__pycache__'))
        shutil.copy(os.path.join(FIX,fixture),os.path.join(tmp,fixture))
        os.makedirs(os.path.join(tmp,'BUILD_A2_MANAGERS'),exist_ok=True)
        env=dict(os.environ,SRCVAL_OUT=os.path.join(tmp,'out.json'))
        r=subprocess.run([sys.executable,os.path.join(tmp,'evidence','validate_sources.py')],
                         capture_output=True,text=True,env=env)
        gates={}
        for line in r.stdout.splitlines():
            if ' = ' in line:
                k,_,v=line.partition(' = ')
                if v.strip().isdigit(): gates[k.strip()]=int(v.strip())
        return r.returncode,gates
    finally:
        shutil.rmtree(tmp,ignore_errors=True)

def package_gate_probes():
    """Prove required JSON Schema absence and fixture-02 acceptance both stop builds."""
    tmp=tempfile.mkdtemp()
    try:
        missing_json=os.path.join(tmp,'missing.json')
        disabled=dict(os.environ,ORCHESTRATOR_DISABLE_JSONSCHEMA='1')
        missing=subprocess.run([sys.executable,os.path.join(HERE,'validate_package.py'),
                                '--root',ROOT,'--json',missing_json],capture_output=True,text=True,env=disabled)
        data=json.load(open(missing_json))
        dependency_ok=(missing.returncode!=0 and
                       data['derived'].get('REQUIRED_VALIDATION_DEPENDENCIES_AVAILABLE')=='NO' and
                       data['derived'].get('SCHEMA_VALIDATION_EXECUTED')=='NO' and
                       data['gates'].get('MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED')==1 and
                       data['gates'].get('VALIDATION_DEPENDENCY_FAIL_OPEN')==1)

        broken=os.path.join(tmp,'broken')
        shutil.copytree(ROOT,broken,ignore=shutil.ignore_patterns('__pycache__'))
        schema=os.path.join(broken,'schemas','HostCapabilityReport.schema.json')
        text=open(schema,encoding='utf-8').read()
        text=text.replace('"plugin_supported":{"const":true}', '"plugin_supported":{}', 1)
        open(schema,'w',encoding='utf-8').write(text)
        validator=os.path.join(broken,'evidence','validate_package.py')
        text=open(validator,encoding='utf-8').read()
        text=text.replace("if complete and x.get('plugin_installed') is True and x.get('plugin_supported') is not True: return False\n", '')
        open(validator,'w',encoding='utf-8').write(text)
        build=subprocess.run([sys.executable,os.path.join(broken,'evidence','build_package.py'),
                              '--root',broken,'--out',os.path.join(tmp,'broken.zip')],capture_output=True,text=True)
        build_ok=build.returncode!=0
        return dependency_ok,build_ok
    finally:
        shutil.rmtree(tmp,ignore_errors=True)
def main():
    passed=false_neg=0; rows=[]; bypass=0
    for fx,(exp,gate,why) in sorted(EXPECT.items()):
        code,gates=run_one(fx)
        got='FAIL' if code!=0 else 'PASS'
        # the named gate must be the REASON, not merely the exit code
        gate_ok = True if gate is None else gates.get(gate,0)>0
        ok = got==exp and gate_ok
        if exp=='FAIL' and got=='PASS': false_neg+=1
        if ok: passed+=1
        bypass=max(bypass,gates.get('SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS',0))
        detail='' if gate is None else f" [{gate}={gates.get(gate,0)}]"
        rows.append((fx,exp,got,'OK' if ok else 'MISMATCH',why))
        print(f"{'OK ' if ok else '!! '} {fx:42s} expected={exp:4s} got={got:4s}{detail}  {why}")
    print(f"\nSOURCE_VALIDATOR_REGRESSION_FIXTURES = {len(EXPECT)}")
    print(f"SOURCE_VALIDATOR_REGRESSION_PASSED   = {passed}")
    print(f"SOURCE_VALIDATOR_FALSE_NEGATIVE_FIXTURES = {false_neg}")
    dependency_ok,build_ok=package_gate_probes()
    print(f"SOURCE_VALIDATOR_HEADER_COMMENT_BYPASS = {bypass}")
    print(f"MISSING_REQUIRED_VALIDATION_DEPENDENCY_ACCEPTED = {'0' if dependency_ok else '1'}")
    print(f"VALIDATION_DEPENDENCY_FAIL_OPEN = {'0' if dependency_ok else '1'}")
    print(f"BUILD_FAILS_ON_PACKAGE_GATE_FAILURE = {'YES' if build_ok else 'NO'}")
    json.dump({'total':len(EXPECT),'passed':passed,'false_negatives':false_neg,
               'header_comment_bypass':bypass,'rows':rows,
               'validation_dependency_regression':dependency_ok,
               'build_fails_on_package_gate_failure':build_ok},
              open(os.environ.get('REGRESSION_OUT','/tmp/regression133.json'),'w'),indent=1)
    return 0 if passed==len(EXPECT) and false_neg==0 and bypass==0 and dependency_ok and build_ok else 1
if __name__=='__main__': sys.exit(main())
