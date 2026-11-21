use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-amiga-project-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn file<'a>(made: &'a Made, name: &str) -> &'a str {
    std::str::from_utf8(&made.files.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no {name}")).1).unwrap()
}

fn template(kind: Kind, os: Os) -> &'static Template {
    TEMPLATES.iter().find(|t| t.kind == kind && t.os == os).unwrap()
}

#[test]
fn each_template_makes_a_program_f9_knows_and_three_makefiles() {
    for template in &TEMPLATES {
        let made = make(template, "hello", "20.11.2026");
        let source = file(&made, "hello.c");
        assert_eq!(crate::compilers::c_machine(source.lines()), Some(crate::running::Machine::Amiga), "{}", template.name);
        assert!(source.contains("$VER: hello 1.0 (20.11.2026)"), "a version string: {}", template.name);
        assert!(!source.contains("@NAME@") && !source.contains("@DATE@"));
        for makefile in [MAKEFILE_GCC, MAKEFILE_VBCC, SMAKEFILE] {
            assert!(file(&made, makefile).contains(BEGIN), "{makefile} has kept lines");
        }
        let icon = made.files.iter().any(|(n, _)| n == "hello.info");
        assert_eq!(icon, made.build.workbench, "an icon for a Workbench program only: {}", template.name);
    }
}

#[test]
fn each_compiler_is_given_what_its_documentation_says_for_each_amigaos() {
    let shell13 = make(template(Kind::Shell, Os::Kick13), "hello", "1.1.2026");
    let gcc = file(&shell13, MAKEFILE_GCC);
    assert!(gcc.contains("TARGET = -mcrt=nix13 -m68000\n"), "libnix for 1.3: {gcc}");
    assert!(gcc.contains("LIBS = -lamiga\n"));
    assert!(file(&shell13, MAKEFILE_VBCC).contains("TARGET = +kick13 -cpu=68000\n"));
    assert!(file(&shell13, MAKEFILE_VBCC).contains("LIBS = -lamiga\n"), "no -lauto: a Shell program");
    let wb3 = make(template(Kind::Workbench, Os::Os3), "hello", "1.1.2026");
    assert!(file(&wb3, MAKEFILE_GCC).contains("TARGET = -noixemul -m68000\n"));
    assert!(file(&wb3, MAKEFILE_VBCC).contains("TARGET = +aos68k -cpu=68000\nLIBS = -lauto -lamiga\n"), "vbcc's auto.lib opens a Workbench program's libraries");
    let wb13 = make(template(Kind::Workbench, Os::Kick13), "hello", "1.1.2026");
    assert!(file(&wb13, MAKEFILE_VBCC).contains("LIBS = -lamiga\n"), "1.3's programs open their own libraries");
    let mui = make(template(Kind::Mui, Os::Os3), "hello", "1.1.2026");
    assert!(file(&mui, MAKEFILE_GCC).contains("-m68020"), "MUI 5 needs a 68020");
    assert!(file(&mui, MAKEFILE_VBCC).contains("-cpu=68020"));
    assert!(file(&mui, SMAKEFILE).contains("TARGET = CPU=68020\n"));
    // The rules themselves.
    assert!(gcc.contains("CC = m68k-amigaos-gcc\n"));
    assert!(gcc.contains("\t$(CC) $(TARGET) $(LDFLAGS) -o $@ $(OBJECTS) $(LIBS)\n"), "libraries after the objects");
    assert!(file(&wb3, MAKEFILE_VBCC).contains("CC = vc\nCFLAGS = -c99 -O1\n"));
    let smake = file(&wb3, SMAKEFILE);
    assert!(smake.contains("\tslink LIB:c.o $(OBJECTS) TO $(PROGRAM) LIB $(LIBS) $(LINKFLAGS)\n"), "{smake}");
    assert!(smake.contains("hello.o: hello.c\n\tsc $(CFLAGS) $(TARGET) hello.c OBJNAME=hello.o\n"), "{smake}");
    assert!(smake.contains("LIBS = LIB:sc.lib LIB:amiga.lib\n"));
}

#[test]
fn floating_point_brings_in_the_maths_libraries() {
    assert!(uses_floating_point("double x = 1.5;"));
    assert!(uses_floating_point("#include <math.h>"));
    assert!(uses_floating_point("static float speed;"));
    assert!(!uses_floating_point("// a float in a comment\nint x; /* double */ char *s = \"float\";"));
    assert!(!uses_floating_point("int floating, doubled;"));
    let mut build = make(template(Kind::Shell, Os::Os3), "hello", "1.1.2026").build;
    build.maths = true;
    assert!(makefile_gcc(&build).contains("LIBS = -lm -lamiga\n"));
    assert!(makefile_vbcc(&build).contains("LIBS = -lmieee -lamiga\n"));
    let smake = smakefile(&build);
    assert!(smake.contains("TARGET = CPU=68000 MATH=IEEE\n"));
    assert!(smake.contains("LIBS = LIB:scmieee.lib LIB:sc.lib LIB:amiga.lib\n"), "the maths library before sc.lib, as SAS/C links it");
}

#[test]
fn the_kept_lines_follow_the_files_as_they_come_and_go() {
    let parent = temp("refresh");
    let (project, main) = create(&parent, template(Kind::Shell, Os::Os3), "hello").unwrap();
    let folder = parent.join("hello");
    assert!(project.is_file() && main.is_file());
    let opened = crate::project::Project::load(&project).unwrap();
    assert_eq!(opened.root.as_deref(), Some(folder.as_path()), "a folder project");
    assert!(opened.tasks.iter().any(|t| t.command == "make -f Makefile.gcc"));
    // Someone's own lines outside the kept ones stay.
    let gcc = folder.join(MAKEFILE_GCC);
    let text = std::fs::read_to_string(&gcc).unwrap().replace("CFLAGS = -Os -Wall", "CFLAGS = -O2 -Wall -DDEBUG");
    std::fs::write(&gcc, text).unwrap();
    std::fs::create_dir_all(folder.join("src")).unwrap();
    std::fs::write(folder.join("src").join("maths.c"), "double half(double x) { return x / 2; }\n").unwrap();
    std::fs::write(folder.join("maths.h"), "double half(double x);\n").unwrap();
    let changed = refresh_folder(&folder).unwrap();
    assert_eq!(changed.len(), 3, "all three makefiles");
    let text = std::fs::read_to_string(&gcc).unwrap();
    assert!(text.contains("CFLAGS = -O2 -Wall -DDEBUG"), "the person's own line is kept");
    assert!(text.contains("SOURCES = hello.c src/maths.c\n"), "{text}");
    assert!(text.contains("HEADERS = maths.h\n"));
    assert!(text.contains("LIBS = -lm -lamiga\n"), "floating point is used now");
    let smake = std::fs::read_to_string(folder.join(SMAKEFILE)).unwrap();
    assert!(smake.contains("OBJECTS = hello.o src/maths.o\n"));
    assert!(smake.contains("src/maths.o: src/maths.c maths.h\n\tsc $(CFLAGS) $(TARGET) src/maths.c OBJNAME=src/maths.o\n"));
    assert!(refresh_folder(&folder).unwrap().is_empty(), "nothing more to change");
    // A file taken away goes from them too.
    std::fs::remove_file(folder.join("src").join("maths.c")).unwrap();
    std::fs::remove_file(folder.join("maths.h")).unwrap();
    refresh_folder(&folder).unwrap();
    let text = std::fs::read_to_string(&gcc).unwrap();
    assert!(text.contains("SOURCES = hello.c\n") && text.contains("LIBS = -lamiga\n"), "{text}");
    // F9 builds it as the makefiles do.
    let f9 = f9_build(&main).unwrap();
    assert_eq!(f9.program, "hello");
    assert_eq!(f9.sources, [folder.join("hello.c")]);
    assert_eq!(f9.vbcc.0, ["+aos68k", "-cpu=68000"]);
    assert!(create(&parent, template(Kind::Shell, Os::Os3), "hello").is_err(), "not over a project that's there");
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn sasc_reads_the_smakefile_limoniter_writes() {
    let parent = temp("smake");
    create(&parent, template(Kind::Workbench, Os::Os3), "wbprog").unwrap();
    let folder = parent.join("wbprog");
    std::fs::write(folder.join("extra.c"), "int extra(void) { return 1; }\n").unwrap();
    refresh_folder(&folder).unwrap();
    let read = crate::sasc::read_smakefile(&folder).unwrap();
    assert_eq!(read.name.as_deref(), Some("wbprog"));
    assert_eq!(read.sources, [folder.join("wbprog.c"), folder.join("extra.c")]);
    let _ = std::fs::remove_dir_all(&parent);
}

/// GNU make reads each makefile and runs what it says: here with -n, so the commands are shown, not run, since no
/// Amiga compiler is installed. They're each compiler's documented command line.
#[test]
fn gnu_make_reads_the_makefiles_and_runs_the_documented_commands() {
    if std::process::Command::new("make").arg("--version").output().is_err() {
        return;
    }
    let parent = temp("make");
    create(&parent, template(Kind::Multilingual, Os::Os3), "hello").unwrap();
    let folder = parent.join("hello");
    let run = |args: &[&str]| {
        let out = std::process::Command::new("make").args(args).current_dir(&folder).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let gcc = run(&["-n", "-f", MAKEFILE_GCC]);
    assert_eq!(gcc.lines().collect::<Vec<_>>(), ["m68k-amigaos-gcc -noixemul -m68000 -Os -Wall -c -o hello.o hello.c", "m68k-amigaos-gcc -noixemul -m68000  -o hello hello.o -lamiga"]);
    let vbcc = run(&["-n", "-f", MAKEFILE_VBCC]);
    assert_eq!(vbcc.lines().collect::<Vec<_>>(), ["vc +aos68k -cpu=68000 -c99 -O1 -c -o hello.o hello.c", "vc +aos68k -cpu=68000  -o hello hello.o -lamiga"]);
    let catalogs = run(&["-n", "-B", "-f", MAKEFILE_GCC, "catalogs"]);
    assert!(catalogs.contains("flexcat hello.cd locale/deutsch.ct CATALOG Catalogs/deutsch/hello.catalog"), "{catalogs}");
    assert!(catalogs.contains("flexcat hello.cd locale/italiano.ct CATALOG Catalogs/italiano/hello.catalog"));
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn the_multilingual_template_comes_with_its_catalogs() {
    let made = make(template(Kind::Multilingual, Os::Os3), "hello", "20.11.2026");
    let cd_bytes = &made.files.iter().find(|(n, _)| n == "hello.cd").unwrap().1;
    let cd = crate::amiga_catalog::read_description(&crate::amiga_catalog::decode(cd_bytes));
    assert!(cd.problems.is_empty(), "{:?}", cd.problems);
    let header = file(&made, "hello_strings.h");
    assert_eq!(header, crate::amiga_catalog::c_header(&cd, "hello"));
    assert!(file(&made, "hello.c").contains("#include \"hello_strings.h\""));
    for language in ["deutsch", "italiano"] {
        let ct_bytes = &made.files.iter().find(|(n, _)| *n == format!("locale/{language}.ct")).unwrap().1;
        let ct = crate::amiga_catalog::read_translation(&crate::amiga_catalog::decode(ct_bytes));
        assert!(crate::amiga_catalog::check(&cd, &ct).is_empty(), "{language}: {:?}", crate::amiga_catalog::check(&cd, &ct));
        let catalog = &made.files.iter().find(|(n, _)| *n == format!("Catalogs/{language}/hello.catalog")).unwrap().1;
        let read = crate::amiga_catalog::read_catalog(catalog).unwrap();
        assert_eq!(read.language.as_deref(), Some(language));
        assert_eq!(read.strings.len(), 3);
    }
    // The German is in Latin 1 on the disk, as the Amiga's tools read it: ü is one byte, 0xFC.
    let german = &made.files.iter().find(|(n, _)| n == "locale/deutsch.ct").unwrap().1;
    assert!(german.windows(5).any(|w| w == b"Tsch\xfc"));
    let catalog = crate::amiga_catalog::read_catalog(&made.files.iter().find(|(n, _)| n == "Catalogs/deutsch/hello.catalog").unwrap().1).unwrap();
    assert_eq!(catalog.strings[1].1, b"Tsch\xfcss.\n");
}

#[test]
fn a_workbench_icon_is_a_diskobject() {
    let icon = tool_icon(8192);
    let word = |at: usize| u16::from_be_bytes([icon[at], icon[at + 1]]);
    let long = |at: usize| u32::from_be_bytes([icon[at], icon[at + 1], icon[at + 2], icon[at + 3]]);
    assert_eq!(word(0), 0xE310, "WB_DISKMAGIC");
    assert_eq!(word(2), 1, "WB_DISKVERSION");
    assert_eq!(word(4 + 8), 32, "the gadget's width");
    assert_ne!(long(4 + 18), 0, "it has a GadgetRender image");
    assert_eq!(icon[48], 3, "WBTOOL");
    assert_eq!(long(74), 8192, "the stack it needs");
    // The image follows the 78-byte DiskObject: 32 by 16 in two planes, then its data, two longs a row.
    assert_eq!((word(78 + 4), word(78 + 6), word(78 + 8)), (32, 16, 2));
    assert_eq!(icon.len(), 78 + 20 + 2 * 16 * 4);
}

#[test]
fn an_existing_sasc_project_is_sorted_and_given_makefiles() {
    let folder = temp("import");
    std::fs::write(folder.join("main.c"), "#include <proto/intuition.h>\nint main(void) { OpenWindow(0); return 0; }\n").unwrap();
    std::fs::write(folder.join("util.c"), "int util(void) { return 0; }\n").unwrap();
    std::fs::write(folder.join("unused.c"), "int unused(void) { return 0; }\n").unwrap();
    std::fs::write(folder.join("util.h"), "int util(void);\n").unwrap();
    std::fs::write(folder.join("smakefile"), "OBJS = main.o util.o\nMyProg: $(OBJS)\n\tslink LIB:c.o $(OBJS) TO MyProg LIB LIB:sc.lib LIB:amiga.lib\n").unwrap();
    std::fs::write(folder.join("SCOPTIONS"), "DATA=NEAR\n").unwrap();
    std::fs::write(folder.join("MyProg"), [0, 0, 3, 0xF3, 0, 0, 0, 0]).unwrap();
    std::fs::write(folder.join("MyProg.info"), [0xE3, 0x10]).unwrap();
    std::fs::write(folder.join("MyProg.guide"), "@database MyProg.guide\n").unwrap();
    std::fs::write(folder.join("Install-MyProg"), "(welcome)\n(copyfiles (source \"MyProg\"))\n").unwrap();
    std::fs::write(folder.join("blit.asm"), " move.l d0,d1\n").unwrap();
    std::fs::write(folder.join("main.o"), [0, 0, 3, 0xE7]).unwrap();
    std::fs::write(folder.join("ReadMe"), "Read me\n").unwrap();
    let imported = import(&folder).unwrap();
    let sorted = &imported.sorted;
    assert_eq!(sorted.sources, ["main.c", "unused.c", "util.c"]);
    assert_eq!(sorted.headers, ["util.h"]);
    assert_eq!(sorted.assembler, ["blit.asm"]);
    assert_eq!(sorted.guides, ["MyProg.guide"]);
    assert_eq!(sorted.installer, ["Install-MyProg"]);
    assert_eq!(sorted.programs, ["MyProg"]);
    assert_eq!(sorted.icons, ["MyProg.info"]);
    assert_eq!(sorted.sasc, ["SCOPTIONS", "smakefile"]);
    assert_eq!(sorted.objects, ["main.o"]);
    assert_eq!(sorted.other, ["ReadMe"]);
    assert_eq!(imported.written, [MAKEFILE_GCC, MAKEFILE_VBCC]);
    let gcc = std::fs::read_to_string(folder.join(MAKEFILE_GCC)).unwrap();
    assert!(gcc.contains("PROGRAM = MyProg\nSOURCES = main.c util.c\n"), "the smakefile's files, not unused.c: {gcc}");
    assert!(gcc.contains("workbench=yes"), "it opens a window");
    assert!(std::fs::read_to_string(folder.join("smakefile")).unwrap().starts_with("OBJS"), "the person's smakefile is left alone");
    let project = crate::project::Project::load(&imported.project).unwrap();
    assert!(project.notes.contains("Installer scripts: Install-MyProg"));
    assert!(project.tasks.iter().any(|t| t.command == "smake"));
    // A second import changes nothing that's there.
    assert!(import(&folder).unwrap().written.is_empty());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn names_are_ones_the_amiga_can_keep() {
    assert!(name_problem("hello").is_none());
    assert!(name_problem("my-prog_2").is_none());
    assert!(name_problem("").is_some());
    assert!(name_problem("2fast").is_some());
    assert!(name_problem("has space").is_some());
    assert!(name_problem("a_name_much_too_long_for_it").is_some());
}

/// With the 3.2 NDK (LIMONITER_NDK naming its folder) and the computer's own gcc, each template that uses only
/// the NDK is compiled as far as checking it (-fsyntax-only) against the NDK's prototypes, where gcc not built
/// for the 68000 takes clib's prototypes rather than inline calls; and the 1.3 templates are checked to call
/// only functions the libraries had by version 34 (1.3), by the NDK's SFD files.
#[test]
fn the_templates_compile_against_the_ndk_where_its_here() {
    let Some(ndk) = std::env::var_os("LIMONITER_NDK").map(PathBuf::from) else { return };
    let index = crate::amiga_ndk::Index::read(&ndk).unwrap();
    let dir = temp("ndk");
    for template in TEMPLATES.iter().filter(|t| t.kind != Kind::Mui) {
        let made = make(template, "hello", "20.11.2026");
        for (name, bytes) in &made.files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let out = std::process::Command::new("gcc")
            .args(["-fsyntax-only", "-std=gnu99", "-Wall", "-Wextra", "-Werror", "-Wno-unused-parameter", "-Wno-pointer-to-int-cast", "-Wno-int-to-pointer-cast", "-I"])
            .arg(ndk.join("Include_H"))
            .arg(dir.join("hello.c"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{}: {}", template.name, String::from_utf8_lossy(&out.stderr));
        if template.os == Os::Kick13 {
            let source = file(&made, "hello.c");
            let code: String = source.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            for word in code.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
                if let Some(function) = index.functions.get(word) {
                    assert!(function.version.is_none_or(|v| v <= 34), "{}: {word} came with version {:?}", template.name, function.version);
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
