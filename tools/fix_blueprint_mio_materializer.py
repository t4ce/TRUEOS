from pathlib import Path

p = Path("tools/apply_blueprint_mio_registration.py")
s = p.read_text()
helper_anchor = '''def ins_after(path: str, anchor: str, text: str) -> None:
    rep(path, anchor, anchor + text)
'''
helper = '''def rep_first(path: str, old: str, new: str) -> None:
    p = Path(path)
    source = p.read_text()
    if old not in source:
        raise SystemExit(f"{path}: missing first-match anchor")
    p.write_text(source.replace(old, new, 1))


'''
if helper_anchor not in s:
    raise SystemExit("materializer helper anchor missing")
s = s.replace(helper_anchor, helper + helper_anchor, 1)
call_anchor = '''rep(
    "src/std_abi_shim.rs",
    ''' + "'''    let mut table = OPEN_FILES.lock();\n"
if call_anchor not in s:
    raise SystemExit("materializer write call anchor missing")
s = s.replace(call_anchor, call_anchor.replace("rep(\n", "rep_first(\n", 1), 1)
p.write_text(s)
print("Blueprint Mio materializer made function-order deterministic")
