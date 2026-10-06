use std::fs;
#[test]
fn copies_sources_without_touching_original() {
    let root = std::env::temp_dir().join(format!(
        "state-drill-copy-{}",
        comandos_store::migrate::journal::new_id(0).unwrap()
    ));
    let source = root.join("source");
    let dest = root.join("dest");
    fs::create_dir_all(source.join(".claude/hooks")).unwrap();
    fs::create_dir(&dest).unwrap();
    let path = source.join(".claude/hooks/snippets.json");
    fs::write(&path, b"[1]\n").unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    comandos_store::migrate::drill::copy_home(&source, &dest).unwrap();
    assert_eq!(
        fs::read(dest.join(".claude/hooks/snippets.json")).unwrap(),
        b"[1]\n"
    );
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), before);
    assert!(!source.join(".claude/hooks/snippets.json.lock").exists());
    fs::remove_dir_all(root).unwrap();
}
