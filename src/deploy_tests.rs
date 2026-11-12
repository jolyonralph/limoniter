use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-deploy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bytes(data: &[u8]) -> Payload {
    Payload::Bytes(Arc::new(data.to_vec()))
}

#[test]
fn devices_are_kept_in_the_settings_and_read_back() {
    let devices = [
        Device { name: "Gotek".into(), place: Place::Card { label: "GOTEK".into(), folder: "GAMES/WIP".into() }, preset: None, eject: true },
        Device { name: "Share".into(), place: Place::Folder { path: PathBuf::from("/srv/tnfs") }, preset: Some("atari-8bit".into()), eject: false },
        Device { name: "MiSTer".into(), place: Place::Sftp { address: "sftp://root@mister/media/fat".into(), key_file: String::new() }, preset: None, eject: false },
        Device { name: "U64 FTP".into(), place: Place::Ftp { host: "u64".into(), port: 21, user: String::new(), folder: "/Usb0/wip".into() }, preset: Some("commodore".into()), eject: false },
        Device { name: "Next".into(), place: Place::NextSync { folder: "/dot".into() }, preset: None, eject: false },
        Device { name: "U64".into(), place: Place::Ultimate { host: "192.168.1.64".into() }, preset: None, eject: false },
        Device { name: "VICE".into(), place: Place::Command { line: "x64sc -autostart {file}".into() }, preset: None, eject: false },
    ];
    for device in &devices {
        let line = device.to_setting();
        assert!(!line.to_lowercase().contains("password"));
        let value = line.trim_end().strip_prefix("send_device=").unwrap();
        assert_eq!(Device::from_setting(value).as_ref(), Some(device));
        assert_eq!(Place::blank(device.place.kind()).kind(), device.place.kind());
    }
    assert_eq!(Device::from_setting("half"), None);
}

#[test]
fn a_device_says_what_it_still_needs() {
    let mut device = Device { name: "Card".into(), place: Place::Card { label: String::new(), folder: String::new() }, preset: None, eject: false };
    assert!(device.problem().is_some());
    device.place = Place::Card { label: "SD".into(), folder: String::new() };
    assert_eq!(device.problem(), None);
    device.place = Place::Command { line: "emulator".into() };
    assert!(device.problem().unwrap().contains("{file}"));
    device.place = Place::Sftp { address: "ftp://nope".into(), key_file: String::new() };
    assert!(device.problem().is_some());
    assert!(name_problem("GAME.PRG").is_none());
    assert!(name_problem("a/b").is_some() && name_problem(" ").is_some() && name_problem("x:y").is_some());
}

#[test]
fn plugged_in_cards_are_found_in_the_mount_list() {
    let mounts = "/dev/sda2 / ext4 rw 0 0\n/dev/sdb1 /media/ann/GOTEK vfat rw 0 0\n/dev/sdc1 /run/media/ann/MY\\040CARD vfat rw 0 0\nnone /mnt/wslg tmpfs rw 0 0\n/dev/sdd1 /mnt/usb exfat rw 0 0\n";
    let labels = vec![("/dev/sdd1".to_string(), "NEXT SD".to_string())];
    let drives = linux_drives(mounts, &labels);
    let found: Vec<(&str, String)> = drives.iter().map(|d| (d.label.as_str(), d.root.display().to_string())).collect();
    assert_eq!(found, [("GOTEK", "/media/ann/GOTEK".to_string()), ("MY CARD", "/run/media/ann/MY CARD".to_string()), ("NEXT SD", "/mnt/usb".to_string())]);
    assert_eq!(drives[0].device.as_deref(), Some("/dev/sdb1"));
    assert_eq!(unescape_mount("NEXT\\x20SD"), "NEXT SD");
}

#[test]
fn a_copy_is_written_safely_and_not_over_something_else() {
    let dir = scratch("write");
    let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
    let folder = dir.join("GAMES").join("WIP");
    let first = write_into(&folder, "GAME.BAS", &bytes(b"10 PRINT"), None, false, &done, &stop).unwrap();
    assert_eq!(std::fs::read(folder.join("GAME.BAS")).unwrap(), b"10 PRINT");
    assert_eq!(first.size, 8);
    assert_eq!(done.load(Ordering::Relaxed), 8);
    // Sent again, with the file as it was left: replaced, and no temporary file left behind.
    write_into(&folder, "GAME.BAS", &bytes(b"10 PRINT \"HI\""), Some(Some(first)), false, &done, &stop).unwrap();
    assert_eq!(std::fs::read(folder.join("GAME.BAS")).unwrap(), b"10 PRINT \"HI\"");
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    // Something else changed it, or it was never sent from here: left alone unless told.
    assert_eq!(write_into(&folder, "GAME.BAS", &bytes(b"x"), Some(Some(first)), false, &done, &stop), Err(Failure::Changed));
    assert_eq!(write_into(&folder, "GAME.BAS", &bytes(b"x"), None, false, &done, &stop), Err(Failure::AlreadyThere));
    write_into(&folder, "GAME.BAS", &bytes(b"x"), None, true, &done, &stop).unwrap();
    assert_eq!(std::fs::read(folder.join("GAME.BAS")).unwrap(), b"x");
    // A big file on disk is copied as it is.
    let big = dir.join("big.bin");
    std::fs::write(&big, vec![7u8; CHUNK * 3 + 5]).unwrap();
    write_into(&folder, "BIG.BIN", &Payload::File(big), None, false, &done, &stop).unwrap();
    assert_eq!(std::fs::metadata(folder.join("BIG.BIN")).unwrap().len(), (CHUNK * 3 + 5) as u64);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nextsync_packing_comes_back_the_same() {
    let mut cases: Vec<Vec<u8>> = vec![Vec::new(), vec![1], vec![1, 1], vec![1, 1, 1], vec![0; 20000], b"abcabc".to_vec(), [vec![5; 3], vec![1, 2, 3], vec![9; 300]].concat()];
    // Long stretches with no runs, cut at 8192 bytes.
    cases.push((0..20000u32).map(|i| (i % 251) as u8).collect());
    let mut seed = 12345u32;
    cases.push(
        (0..50000)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                if seed >> 28 < 6 { 0 } else { (seed >> 16) as u8 }
            })
            .collect(),
    );
    for data in &cases {
        let packed = next_pack(data);
        assert_eq!(next_unpack(&packed).as_deref(), Some(data.as_slice()), "{} bytes", data.len());
    }
    // As NextSync's own server packs them.
    assert_eq!(next_pack(&[1, 2, 3, 3, 3, 3, 4]), [2, 1, 2, 4, 3, 1, 4]);
    assert_eq!(next_pack(&[7, 7, 7, 1]), [0, 3, 7, 1, 1]);
    assert_eq!(next_pack(&[0; 300]), [0, 0xFF, 1, 44, 0]);
}

/// Plays the Next's side of NextSync, as `.sync` does.
fn fetch_as_the_next(port: u16, hello: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let ask = |stream: &mut TcpStream, what: &[u8]| -> Vec<u8> {
        stream.write_all(what).unwrap();
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).unwrap();
        let mut rest = vec![0u8; u16::from_be_bytes(head) as usize - 2];
        stream.read_exact(&mut rest).unwrap();
        let payload = rest[..rest.len() - 3].to_vec();
        let (mut sum0, mut sum1) = (0u8, 0u8);
        for &b in &payload {
            sum0 ^= b;
            sum1 = sum1.wrapping_add(sum0);
        }
        assert_eq!(&rest[rest.len() - 3..rest.len() - 1], &[sum0, sum1]);
        payload
    };
    let version = ask(&mut stream, hello);
    let packed = version == b"NextSync4";
    let mut files = Vec::new();
    loop {
        let header = ask(&mut stream, b"Next");
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        if header == [0, 0, 0, 0, 0] {
            break;
        }
        let name = String::from_utf8(header[5..5 + header[4] as usize].to_vec()).unwrap();
        let mut data = Vec::new();
        let mut retried = false;
        loop {
            let complete = if packed { next_unpack(&data).is_some_and(|d| d.len() >= size) } else { data.len() >= size };
            if complete {
                break;
            }
            let chunk = ask(&mut stream, b"Get");
            if !retried {
                // A garbled packet is asked for again.
                retried = true;
                assert_eq!(ask(&mut stream, b"Retry"), chunk);
            }
            data.extend(chunk);
        }
        files.push((name, if packed { next_unpack(&data).unwrap() } else { data }));
    }
    assert_eq!(ask(&mut stream, b"Bye"), b"Later");
    files
}

#[test]
fn the_next_fetches_what_is_waiting_for_it() {
    let server = NextSync::start(0).unwrap();
    let program: Vec<u8> = (0..5000u32).map(|i| if i % 700 < 300 { 0 } else { (i % 13) as u8 }).collect();
    server.offer(Waiting { path: next_path("/dot", "hello"), data: Arc::new(program.clone()), number: 1 });
    server.offer(Waiting { path: next_path("games/mine", "GAME.BAS"), data: Arc::new(b"old".to_vec()), number: 2 });
    server.offer(Waiting { path: next_path("games/mine", "GAME.BAS"), data: Arc::new(b"10 PRINT".to_vec()), number: 3 });
    for hello in [&b"Sync4"[..], &b"Sync3"[..]] {
        let files = fetch_as_the_next(server.port, hello);
        assert_eq!(files, [("/dot/hello".to_string(), program.clone()), ("/games/mine/GAME.BAS".to_string(), b"10 PRINT".to_vec())]);
        // Fetched, so nothing waits now; offered again for the second way of asking.
        let mut shared = lock(&server.shared);
        assert!(shared.waiting.is_empty());
        assert_eq!(std::mem::take(&mut shared.fetched).len(), 2);
        drop(shared);
        server.offer(Waiting { path: "/dot/hello".into(), data: Arc::new(program.clone()), number: 4 });
        server.offer(Waiting { path: "/games/mine/GAME.BAS".into(), data: Arc::new(b"10 PRINT".to_vec()), number: 5 });
    }
    assert_eq!(next_path("", "a"), "/a");
    assert_eq!(next_path("c:/apps/", "a"), "c:/apps/a");
}

/// A small FTP server that keeps what's stored, to check the client against.
fn fake_ftp(expect_password: &'static str) -> (u16, Arc<Mutex<Vec<(String, Vec<u8>)>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let stored = Arc::new(Mutex::new(Vec::new()));
    let keep = stored.clone();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut out = stream;
        let mut folder = String::from("/");
        let mut data: Option<TcpListener> = None;
        out.write_all(b"220-Welcome\r\n220 Ready\r\n").unwrap();
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let command = line.trim_end().to_string();
            line.clear();
            let (verb, arg) = command.split_once(' ').unwrap_or((&command, ""));
            let reply = match verb {
                "USER" => "331 Password please".to_string(),
                "PASS" if arg == expect_password => "230 In".to_string(),
                "PASS" => "530 No".to_string(),
                "TYPE" => "200 Binary".to_string(),
                "CWD" if arg == "/" || arg == "/Usb0" || arg == "Usb0" => {
                    folder = "/Usb0".into();
                    "250 Ok".to_string()
                }
                "CWD" if arg == "wip" && folder.ends_with("wip") => "250 Ok".to_string(),
                "CWD" => "550 No such folder".to_string(),
                "MKD" => {
                    folder = format!("{folder}/{arg}");
                    "257 Made".to_string()
                }
                "EPSV" => "500 Unknown".to_string(),
                "PASV" => {
                    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                    let p = listener.local_addr().unwrap().port();
                    data = Some(listener);
                    format!("227 Entering Passive Mode (10,0,0,9,{},{})", p >> 8, p & 255)
                }
                "STOR" => {
                    out.write_all(b"150 Go\r\n").unwrap();
                    let (mut conn, _) = data.take().unwrap().accept().unwrap();
                    let mut got = Vec::new();
                    conn.read_to_end(&mut got).unwrap();
                    keep.lock().unwrap().push((format!("{folder}/{arg}"), got));
                    "226 Stored".to_string()
                }
                "QUIT" => {
                    out.write_all(b"221 Bye\r\n").unwrap();
                    break;
                }
                _ => "502 No".to_string(),
            };
            out.write_all(format!("{reply}\r\n").as_bytes()).unwrap();
        }
    });
    (port, stored)
}

#[test]
fn a_file_is_sent_by_ftp_into_a_folder_it_makes() {
    let (port, stored) = fake_ftp("secret");
    let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
    let data: Vec<u8> = (0..300_000u32).map(|i| i as u8).collect();
    ftp_send("127.0.0.1", port, "", "secret", "/Usb0/wip", "GAME.PRG", &Payload::Bytes(Arc::new(data.clone())), &done, &stop).unwrap();
    assert_eq!(*stored.lock().unwrap(), [("/Usb0/wip/GAME.PRG".to_string(), data)]);
    let (port, _) = fake_ftp("secret");
    let refused = ftp_send("127.0.0.1", port, "me", "wrong", "", "x", &bytes(b"x"), &done, &stop).unwrap_err();
    assert!(refused.contains("sign-in"), "{refused}");
}

/// Answers one HTTP request as an Ultimate does, and hands back what was asked.
fn fake_ultimate(status: &'static str, body: &'static str) -> (u16, std::sync::mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tell, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut got = Vec::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = stream.read(&mut buffer).unwrap_or(0);
            got.extend_from_slice(&buffer[..n]);
            let text = String::from_utf8_lossy(&got).to_string();
            if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                let length: usize = head.lines().find_map(|l| l.strip_prefix("Content-Length: ")).and_then(|v| v.parse().ok()).unwrap_or(0);
                if got.len() - (head.len() + 4) >= length || n == 0 {
                    let _ = rest;
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}").as_bytes()).unwrap();
        tell.send(got).unwrap();
    });
    (port, heard)
}

#[test]
fn an_ultimate_is_told_to_run_a_program_or_mount_a_disk() {
    let done = AtomicU64::new(0);
    let (port, heard) = fake_ultimate("200 OK", "{\"errors\":[]}");
    let note = ultimate_send(&format!("127.0.0.1:{port}"), "pw", "game.prg", &bytes(&[1, 8, 0x60]), &done).unwrap();
    assert!(note.contains("started"));
    let request = heard.recv().unwrap();
    let text = String::from_utf8_lossy(&request);
    assert!(text.starts_with("POST /v1/runners:run_prg HTTP/1.1\r\n"), "{text}");
    assert!(text.contains("X-Password: pw\r\n") && text.contains("Content-Length: 3\r\n"));
    assert!(request.ends_with(&[1, 8, 0x60]));

    let (port, heard) = fake_ultimate("200 OK", "{\"errors\":[]}");
    ultimate_send(&format!("127.0.0.1:{port}"), "", "disk.D64", &bytes(b"DISK"), &done).unwrap();
    let text = String::from_utf8_lossy(&heard.recv().unwrap()).to_string();
    assert!(text.starts_with("POST /v1/drives/a:mount?mode=readwrite HTTP/1.1"));
    assert!(text.contains("filename=\"disk.D64\"") && text.contains("DISK\r\n--limoniter-") && !text.contains("X-Password"));

    let (port, _) = fake_ultimate("200 OK", "{\"errors\":[\"Not a valid PRG\"]}");
    assert!(ultimate_send(&format!("127.0.0.1:{port}"), "", "x.prg", &bytes(b"x"), &done).unwrap_err().contains("Not a valid PRG"));
    let (port, _) = fake_ultimate("403 Forbidden", "");
    assert!(ultimate_send(&format!("127.0.0.1:{port}"), "", "x.prg", &bytes(b"x"), &done).unwrap_err().contains("password"));
    assert!(ultimate_send("127.0.0.1:9", "", "notes.txt", &bytes(b"x"), &done).unwrap_err().contains("wouldn't know"));
}

#[test]
fn a_command_line_gets_the_copy() {
    let dir = scratch("command");
    let file = dir.join("my game.bas");
    std::fs::write(&file, "10 PRINT").unwrap();
    let filled = fill_command("emu -autostart {file} --name {name}", &file);
    assert!(filled.contains("my game.bas") && filled.matches("my game.bas").count() == 2);
    let line = if cfg!(windows) { "type {file}" } else { "cat {file}" };
    assert_eq!(run_command(line, &file).unwrap(), "10 PRINT");
    let failing = if cfg!(windows) { "type {folder}\\missing.txt" } else { "cat {folder}/missing.txt" };
    assert!(run_command(failing, &file).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_send_to_a_folder_runs_in_the_background() {
    let dir = scratch("job");
    let device = Device { name: "Share".into(), place: Place::Folder { path: dir.join("out") }, preset: None, eject: false };
    let sending = Sending { device, name: "A.TXT".into(), payload: bytes(b"hello"), expected: None, force: false, password: String::new(), sftp_secret: None, known_hosts: Default::default(), scratch: dir.join("scratch") };
    let job = Job::start(7, sending);
    let outcome = loop {
        if let Some(outcome) = job.take() {
            break outcome;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let Outcome::Sent { stamp, place, .. } = outcome else { panic!("{outcome:?}") };
    assert_eq!(stamp.unwrap().size, 5);
    assert!(place.ends_with("A.TXT"));
    assert_eq!(std::fs::read(dir.join("out").join("A.TXT")).unwrap(), b"hello");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Checks the packing against NextSync's own server: LIMONITER_NEXTSYNC_CASES names a file of cases made with its
/// compress_rle, each a 4-byte length, the data, a 4-byte length and what it packed to.
#[test]
#[ignore]
fn packing_matches_nextsyncs_own_server() {
    let cases = std::fs::read(std::env::var("LIMONITER_NEXTSYNC_CASES").unwrap()).unwrap();
    let (mut at, mut same, mut count) = (0, 0, 0);
    let take = |at: &mut usize| {
        let n = u32::from_be_bytes(cases[*at..*at + 4].try_into().unwrap()) as usize;
        *at += 4 + n;
        &cases[*at - n..*at]
    };
    while at < cases.len() {
        let data = take(&mut at);
        let theirs = take(&mut at);
        assert_eq!(next_unpack(theirs).as_deref(), Some(data), "their packing unpacks the same");
        count += 1;
        same += usize::from(next_pack(data) == theirs);
    }
    assert_eq!(same, count, "{same} of {count} packed byte for byte the same");
}

/// Lists the drives on this computer as Send to device sees them, and if LIMONITER_CARD names one by its label,
/// sends a file to it, finds it there, and takes it away again.
#[test]
#[ignore = "looks at this computer's own drives"]
fn the_drives_here_are_found_by_label() {
    let drives = drives();
    for drive in &drives {
        eprintln!("{:<12} {:<24} removable={}", drive.root.display(), drive.label, drive.removable);
    }
    let Ok(label) = std::env::var("LIMONITER_CARD") else {
        return;
    };
    let card = find_card(&label.to_lowercase()).expect("the card is found whatever the case of its label");
    let folder = card.root.join("limoniter-send-test");
    let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
    write_into(&folder, "TEST.TXT", &bytes(b"sent"), None, true, &done, &stop).unwrap();
    assert_eq!(std::fs::read(folder.join("TEST.TXT")).unwrap(), b"sent");
    std::fs::remove_dir_all(&folder).unwrap();
}

/// Sends to a real FTP server. LIMONITER_FTP is host:port, LIMONITER_FTP_USER and LIMONITER_FTP_PASSWORD sign in,
/// and LIMONITER_FTP_ROOT is the server's top folder as this computer sees it.
#[test]
#[ignore = "needs an FTP server named in LIMONITER_FTP"]
fn a_real_ftp_server_takes_a_file() {
    let (Ok(address), Ok(user), Ok(password), Ok(root)) = (std::env::var("LIMONITER_FTP"), std::env::var("LIMONITER_FTP_USER"), std::env::var("LIMONITER_FTP_PASSWORD"), std::env::var("LIMONITER_FTP_ROOT")) else {
        return;
    };
    let (host, port) = address.rsplit_once(':').unwrap();
    let root = PathBuf::from(root);
    let _ = std::fs::remove_dir_all(root.join("games"));
    let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
    let data: Vec<u8> = (0..1_000_000u32).map(|i| (i * 7) as u8).collect();
    ftp_send(host, port.parse().unwrap(), &user, &password, "/games/new", "BIG.PRG", &Payload::Bytes(Arc::new(data.clone())), &done, &stop).unwrap();
    assert_eq!(std::fs::read(root.join("games").join("new").join("BIG.PRG")).unwrap(), data);
    ftp_send(host, port.parse().unwrap(), &user, &password, "games/new", "BIG.PRG", &bytes(b"smaller"), &done, &stop).unwrap();
    assert_eq!(std::fs::read(root.join("games").join("new").join("BIG.PRG")).unwrap(), b"smaller");
    let wrong = ftp_send(host, port.parse().unwrap(), &user, "wrong", "", "X", &bytes(b"x"), &done, &stop).unwrap_err();
    assert!(wrong.contains("sign-in"), "{wrong}");
    std::fs::remove_dir_all(root.join("games")).unwrap();
}

/// Sends to a real SFTP server, as File, Send to device does. LIMONITER_SFTP is sftp://user@host:port/folder,
/// LIMONITER_SFTP_KEY a private key it accepts, and LIMONITER_SFTP_ROOT the server's top folder as seen here.
#[test]
#[ignore = "needs an SFTP server named in LIMONITER_SFTP"]
fn a_real_sftp_server_takes_a_file() {
    let (Ok(url), Ok(key), Ok(root)) = (std::env::var("LIMONITER_SFTP"), std::env::var("LIMONITER_SFTP_KEY"), std::env::var("LIMONITER_SFTP_ROOT")) else {
        return;
    };
    let root = PathBuf::from(root);
    let _ = std::fs::remove_file(root.join("SENT.BAS"));
    let device = Device { name: "Pi".into(), place: Place::Sftp { address: url, key_file: key }, preset: None, eject: false };
    let mut sending = Sending { device, name: "SENT.BAS".into(), payload: bytes(b"10 PRINT"), expected: None, force: false, password: String::new(), sftp_secret: None, known_hosts: Default::default(), scratch: scratch("sftp") };
    let run = |sending: &Sending| {
        let job = Job::start(1, sending.clone());
        loop {
            if let Some(outcome) = job.take() {
                return outcome;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    // A server not met before is refused until its key is trusted.
    let Outcome::Failed(Failure::Refused(crate::remote::Refused::UnknownHost(print))) = run(&sending) else { panic!("an unknown server is refused") };
    let (site, _) = crate::remote::parse_url(match &sending.device.place { Place::Sftp { address, .. } => address, _ => unreachable!() }).unwrap();
    sending.known_hosts.trust(&site, &print);
    let Outcome::Sent { stamp: Some(first), .. } = run(&sending) else { panic!("sent") };
    assert_eq!(std::fs::read(root.join("SENT.BAS")).unwrap(), b"10 PRINT");
    // Sent again as it was left: replaced. Changed there by someone else: asked about.
    sending.expected = Some(Some(first));
    sending.payload = bytes(b"10 PRINT 2");
    let Outcome::Sent { stamp: Some(second), .. } = run(&sending) else { panic!("sent again") };
    std::fs::write(root.join("SENT.BAS"), b"theirs, longer").unwrap();
    sending.expected = Some(Some(second));
    assert!(matches!(run(&sending), Outcome::Failed(Failure::Changed)));
    sending.force = true;
    assert!(matches!(run(&sending), Outcome::Sent { .. }));
    assert_eq!(std::fs::read(root.join("SENT.BAS")).unwrap(), b"10 PRINT 2");
    std::fs::remove_file(root.join("SENT.BAS")).unwrap();
}
