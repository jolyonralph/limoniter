use super::*;
use std::collections::HashMap;

fn testmedia(path: &str) -> Option<Vec<u8>> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../testmedia").join(path)).ok()
}

/// Files kept in memory, read as the plan reads them.
fn files(list: &[(&str, &[u8])]) -> impl Fn(&Path) -> Result<Vec<u8>, String> + use<> {
    let map: HashMap<PathBuf, Vec<u8>> = list.iter().map(|(n, b)| (PathBuf::from(n), b.to_vec())).collect();
    move |p: &Path| map.get(p).cloned().ok_or_else(|| format!("no {}", p.display()))
}

fn piece(name: &str, file: &str, sector: usize, load: u32, start: bool) -> Piece {
    Piece { name: name.into(), file: file.into(), sector, load, start, loaded: true }
}

fn long(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn the_checksum_is_kickstart_s_as_real_disks_have_it() {
    let mut checked = 0;
    for path in ["amiga-floppy-fredfish-801/amiga-floppy-fredfish-801.adf", "gloom-deluxe-demo/Gloom_Deluxe_Demo.adf", "limoniter-unpacked/TNF-TIT-megademo-1988.adf", "studemo-7-by-stuart-tyrrell-1991/StuDemo 7.adf", "virtual-karting-demo/VirtualKarting2Demo.adf"] {
        let Some(disk) = testmedia(path) else { continue };
        // A disk made bootable: one that was only formatted has "DOS" but no checksum or code.
        if &disk[..3] != b"DOS" || long(&disk, 4) == 0 {
            continue;
        }
        assert_eq!(checksum(&disk[..BOOT_BYTES]), long(&disk, 4), "{path}");
        checked += 1;
    }
    if checked == 0 {
        return;
    }
    assert!(checked >= 2);
}

#[test]
fn the_example_boot_block_assembles_into_its_1024_bytes_with_room_to_spare() {
    let plan = Plan { pieces: vec![piece("Main", "main.s", 2, 0x40000, true)], ..Plan::default() };
    let made = vec![Some(Made { bytes: vec![0x4E, 0x75], load: 0x40000, entry: 0x40000 })];
    let block = make_boot(&example_boot_source(), &plan, &made).unwrap();
    assert_eq!(block.len(), BOOT_BYTES);
    assert_eq!(&block[..4], b"DOS\0");
    assert_eq!(long(&block, 8), 880);
    // The whole block adds up, with carries, to $FFFFFFFF.
    let mut sum: u32 = 0;
    for chunk in block.chunks(4) {
        let (s, c) = sum.overflowing_add(u32::from_be_bytes(chunk.try_into().unwrap()));
        sum = s + u32::from(c);
    }
    assert_eq!(sum, 0xFFFF_FFFF);
    // The code starts with MOVE.L A1,A5 and the table holds the start, then sector 2 (byte 1024), one sector,
    // to $40000, then the end.
    assert_eq!(&block[12..14], &[0x2A, 0x49]);
    let size = boot_size(&example_boot_source(), &plan, &made).unwrap();
    assert!(size < 200, "{size}");
    let table = size - 20;
    assert_eq!([long(&block, table), long(&block, table + 4), long(&block, table + 8), long(&block, table + 12), long(&block, table + 16)], [0x40000, 1024, 512, 0x40000, 0]);
}

#[test]
fn a_boot_block_too_big_for_its_1024_bytes_is_refused_and_says_by_how_much() {
    let source = format!("{}\n\t\tds.b\t1000\n", example_boot_source());
    let plan = Plan::default();
    let problem = make_boot(&source, &plan, &[]).unwrap_err();
    assert!(problem.contains("more than the 1024"), "{problem}");
    // So is one that doesn't start "DOS".
    let wrong = example_boot_source().replace("'DOS',0", "'XYZ',0");
    assert!(make_boot(&wrong, &plan, &[]).unwrap_err().contains("DOS"));
}

#[test]
fn the_table_goes_between_the_marked_lines_of_a_source_of_the_person_s_own() {
    let own = "\t\torg\t0\n\t\tdc.b\t'DOS',0\n\t\tdc.l\t0,880\n\t\tlea\tpieces(pc),a0\n\t\tmove.l\tstart(pc),a1\n\t\tjmp\t(a1)\n; >>>pieces\nold table\n; <<<pieces\n; mine after it\n";
    let plan = Plan { pieces: vec![piece("A", "a.bin", 10, 0x50000, true), piece("B", "b.bin", 20, 0x60000, false)], ..Plan::default() };
    let made = vec![Some(Made { bytes: vec![1; 600], load: 0x50000, entry: 0x50000 }), Some(Made { bytes: vec![2; 10], load: 0x60000, entry: 0x60000 })];
    let source = with_table(own, &table_source(&plan, &made));
    assert!(!source.contains("old table") && source.contains("; mine after it"));
    assert!(source.contains("dc.l    5120,1024,$50000") && source.contains("dc.l    10240,512,$60000") && source.contains("start:  dc.l    $50000"), "{source}");
    assert!(make_boot(own, &plan, &made).is_ok());
}

#[test]
fn pieces_are_assembled_at_their_addresses_and_written_to_their_sectors() {
    // One with its own ORG and an END label to start at, one without (put at the plan's address), and data.
    let main = b"\t\torg\t$50000\ndata:\tdc.w\t1,2\nentry:\tmove.w\t#$0f0,$dff180\n\t\tbra.s\tentry\n\t\tend\tentry\n";
    let other = b"\t\tlea\there(pc),a0\nhere:\trts\n";
    let data = vec![0xAAu8; 1300];
    let read = files(&[("main.s", main), ("other.asm", other), ("music.bin", &data)]);
    let plan = Plan { pieces: vec![piece("Main", "main.s", 2, 0, true), piece("Other", "other.asm", 40, 0x60000, false), piece("Music", "music.bin", 100, 0x20000, false)], ..Plan::default() };
    let built = build(&plan, &example_boot_source(), None, &read).unwrap();
    assert_eq!(built.image.len(), 901_120);
    let made: Vec<&Made> = built.made.iter().map(|m| m.as_ref().unwrap()).collect();
    assert_eq!((made[0].load, made[0].entry), (0x50000, 0x50004));
    assert_eq!(made[1].load, 0x60000);
    assert_eq!(made[2].sectors(), 3);
    assert_eq!(&built.image[1024..1028], &[0, 1, 0, 2]);
    assert_eq!(&built.image[40 * 512..40 * 512 + 4], &[0x41, 0xFA, 0x00, 0x02], "LEA here(PC),A0");
    assert!(built.image[100 * 512..100 * 512 + 1300].iter().all(|&b| b == 0xAA));
    assert!(built.image[100 * 512 + 1300..103 * 512].iter().all(|&b| b == 0));
    // The boot block jumps to Main's END label.
    let boot = &built.image[..BOOT_BYTES];
    let table = built.boot_size - 4 - 3 * 12 - 4;
    assert_eq!(long(boot, table), 0x50004);
    assert_eq!(checksum(boot), long(boot, 4));
    // The map: the boot block, then each piece's sectors.
    let owners = map(&plan, &built.made);
    assert_eq!((owners[0], owners[1], owners[2], owners[3]), (Owner::Boot, Owner::Boot, Owner::Piece(0), Owner::Free));
    assert_eq!(owners[100..103], [Owner::Piece(2); 3]);
    assert_eq!(owners[103], Owner::Free);
    assert_eq!(plan.next_free(&owners, 2), Some(3));
    assert_eq!(plan.next_free(&owners, 40), Some(41));
}

#[test]
fn clashes_the_boot_block_the_disk_s_end_and_the_start_are_all_checked() {
    let made = |n: usize| Some(Made { bytes: vec![0; n * 512], load: 0, entry: 0 });
    let plan = Plan { pieces: vec![piece("A", "a", 5, 0, true), piece("B", "b", 7, 0, false), piece("C", "c", 1, 0, false), piece("D", "d", 1758, 0, false)], ..Plan::default() };
    let problems = place_problems(&plan, &[made(3), made(1), made(1), made(4)]);
    assert!(problems.iter().any(|p| p.contains("A and B")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("C starts at sector 1")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("D needs sectors 1758 to 1761")), "{problems:?}");
    assert_eq!(map(&plan, &[made(3), made(1), made(1), made(4)])[7], Owner::Clash);
    let none = Plan { pieces: vec![piece("A", "a", 5, 0, false)], ..Plan::default() };
    assert!(place_problems(&none, &[made(1)]).iter().any(|p| p.contains("No piece is marked to start")));
}

#[test]
fn a_plan_is_kept_as_text_and_read_back() {
    let folder = Path::new("/disks/demo");
    let plan = Plan {
        density: Density::High,
        pieces: vec![piece("Main", "/disks/demo/main.s", 2, 0x40000, true), Piece { loaded: false, ..piece("Level 2", "/elsewhere/level2.bin", 300, 0x70000, false) }],
        boot: Some("/disks/demo/boot.s".into()),
    };
    let text = plan.to_text(Some(folder));
    assert!(text.contains("piece = Main\tmain.s\t2\t$40000\tstart") && text.contains("/elsewhere/level2.bin") && text.contains("boot = boot.s"), "{text}");
    assert_eq!(Plan::from_text(&text, Some(folder)).unwrap(), plan);
    assert!(Plan::from_text("piece = nothing much", None).is_err());
    assert_eq!(place(Density::Double, 0), (0, 0, 0));
    assert_eq!(place(Density::Double, 11), (0, 1, 0));
    assert_eq!(place(Density::Double, 1759), (79, 1, 10));
    assert_eq!((number("$40000"), number("0x40000"), number("262144")), (Some(0x40000), Some(0x40000), Some(0x40000)));
}

/// The disk boots on a real (emulated) Amiga: FS-UAE with a Kickstart boots it, the boot block loads a program
/// at sector 2 and a message far along the disk, at sector 1500, and the program sends the message out of the
/// serial port, which FS-UAE writes to a file. Run with LIMONITER_AMIGA_BOOT=path/to/fs-uae and
/// LIMONITER_KICKSTART=path/to/rom (FS-UAE needs a display: xvfb-run on a machine with none).
#[test]
#[ignore]
fn a_disk_boots_loads_its_pieces_and_runs_on_an_amiga() {
    let (Some(emulator), Some(rom)) = (std::env::var_os("LIMONITER_AMIGA_BOOT"), std::env::var_os("LIMONITER_KICKSTART")) else { return };
    let program = b"
SERDATR\tequ\t$dff018
SERDAT\tequ\t$dff030
SERPER\tequ\t$dff032
\t\torg\t$40000
start:\tmove.w\t#$7fff,$dff09a\t\t; no interrupts: this owns the machine now
\t\tmove.w\t#372,SERPER\t\t; 9600 baud on a PAL Amiga
\t\tlea\tmessage,a0
.next:\tmove.b\t(a0)+,d0
\t\tbeq.s\t.done
.wait:\tbtst\t#13-8,SERDATR\t\t; until the last byte's gone
\t\tbeq.s\t.wait
\t\tand.w\t#$ff,d0
\t\tor.w\t#$100,d0\t\t\t; a stop bit
\t\tmove.w\td0,SERDAT
\t\tbra.s\t.next
.done:\tmove.w\t#$00f0,$dff180\t\t; green
\t\tbra.s\t.done
message\tequ\t$60000
";
    let message = b"Booted by Limoniter, read from sector 1500\n\0";
    let read = files(&[("program.s", program), ("message.bin", message)]);
    let plan = Plan { pieces: vec![piece("Program", "program.s", 2, 0, true), piece("Message", "message.bin", 1500, 0x60000, false)], ..Plan::default() };
    let built = build(&plan, &example_boot_source(), None, &read).unwrap();
    let folder = std::env::temp_dir().join(format!("limoniter-bootdisk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let disk = folder.join("boot.adf");
    std::fs::write(&disk, &built.image).unwrap();
    let serial = folder.join("serial.txt");
    std::fs::write(&serial, b"").unwrap();
    let config = folder.join("boot.fs-uae");
    let rom = PathBuf::from(rom);
    std::fs::write(
        &config,
        format!(
            "[fs-uae]\namiga_model = {}\nkickstart_file = {}\nfloppy_drive_0 = {}\nserial_port = {}\nfloppy_drive_speed = 0\nbase_dir = {}\nfullscreen = 0\nautomatic_input_grab = 0\ninitial_input_grab = 0\n",
            std::env::var("LIMONITER_AMIGA_MODEL").unwrap_or_else(|_| "A500".into()),
            rom.display(),
            disk.display(),
            serial.display(),
            folder.join("base").display()
        ),
    )
    .unwrap();
    let log = std::fs::File::create(folder.join("fs-uae.log")).unwrap();
    let mut child = crate::platform::command(&emulator).arg(&config).arg("--log_to_stdout=1").stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap();
    let began = std::time::Instant::now();
    let mut said = String::new();
    while began.elapsed() < std::time::Duration::from_secs(40) && !said.contains('\n') {
        std::thread::sleep(std::time::Duration::from_millis(300));
        said = String::from_utf8_lossy(&std::fs::read(&serial).unwrap_or_default()).into_owned();
    }
    let _ = child.kill();
    let _ = child.wait();
    eprintln!("After {:.1}s the serial port said: {said:?}", began.elapsed().as_secs_f32());
    if std::env::var_os("LIMONITER_KEEP").is_some() {
        eprintln!("Kept {}", folder.display());
    }
    assert_eq!(said, "Booted by Limoniter, read from sector 1500\n");
    let _ = std::fs::remove_dir_all(&folder);
}
