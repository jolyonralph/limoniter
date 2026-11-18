use super::*;

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-workspace-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, text).unwrap();
}

#[test]
fn gitignore_rules_are_git_s_own() {
    let m = |pattern: &str, path: &str| rule(pattern).unwrap().regex.is_match(path);
    assert!(m("*.o", "main.o") && m("*.o", "src/deep/main.o") && !m("*.o", "main.c"));
    assert!(m("/build", "build") && !m("/build", "src/build"));
    assert!(m("doc/*.txt", "doc/a.txt") && !m("doc/*.txt", "doc/sub/a.txt"));
    assert!(m("**/logs", "logs") && m("**/logs", "a/b/logs"));
    assert!(m("logs/**", "logs/x/y.txt"));
    assert!(m("a/**/b", "a/b") && m("a/**/b", "a/x/y/b"));
    assert!(m("file?.txt", "file1.txt") && !m("file?.txt", "file10.txt"));
    assert!(m("[abc].c", "b.c") && m("[!abc].c", "d.c") && !m("[!abc].c", "a.c"));
    assert!(rule("# a comment").is_none() && rule("   ").is_none());
    let negated = rule("!keep.o").unwrap();
    assert!(negated.negated && negated.regex.is_match("keep.o"));
    assert!(rule("out/").unwrap().folders_only);
}

#[test]
fn the_index_leaves_out_what_git_would_and_never_goes_into_dot_git() {
    let root = folder("index");
    write(&root, ".gitignore", "*.o\n/build/\n!keep.o\nnode_modules\n");
    write(&root, "src/main.c", "");
    write(&root, "src/main.o", "");
    write(&root, "src/keep.o", "");
    write(&root, "build/out.bin", "");
    write(&root, "src/build/still.c", "");
    write(&root, "node_modules/x/index.js", "");
    write(&root, ".git/HEAD", "");
    write(&root, "lib/.gitignore", "*.tmp\n");
    write(&root, "lib/a.tmp", "");
    write(&root, "lib/a.s", "");
    write(&root, "a.tmp", "");
    let (files, more) = index(&root);
    let names: Vec<String> = files.iter().map(|f| f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect();
    assert!(!more);
    assert_eq!(names, [".gitignore", "a.tmp", "lib/.gitignore", "lib/a.s", "src/build/still.c", "src/keep.o", "src/main.c"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_few_letters_find_the_file_meant() {
    let paths = ["src/app.rs", "src/asm/x86.rs", "src/asm_host.rs", "web/js/main.js", "src/amiga_bootdisk_ui.rs", "README.md"];
    let found = |q: &str| find(q, paths.iter().map(|p| (Path::new(*p), p.to_string())), 3).into_iter().map(|(_, s, _)| s).collect::<Vec<_>>();
    assert_eq!(found("x86")[0], "src/asm/x86.rs");
    assert_eq!(found("asmhost")[0], "src/asm_host.rs");
    assert_eq!(found("bdui")[0], "src/amiga_bootdisk_ui.rs");
    assert_eq!(found("mainjs")[0], "web/js/main.js");
    assert_eq!(found("readme")[0], "README.md");
    assert!(found("zzz").is_empty());
    // Letters together beat letters starting words: host is git_host.rs before git_host_tests.rs's h-o-s and t.
    let hosts = ["src/git_host_tests.rs", "src/git_host.rs", "src/st_host.rs"];
    let first = find("host", hosts.iter().map(|p| (Path::new(*p), p.to_string())), 3);
    assert_eq!(first[0].1, "src/st_host.rs");
    assert_eq!(first[1].1, "src/git_host.rs");
    assert_eq!(first[0].2, [7, 8, 9, 10]);
    // The places matched, to show.
    assert_eq!(score("ah", "asm_host.rs").unwrap().1, [0, 4]);
}
