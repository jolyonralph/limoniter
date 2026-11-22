use super::*;
use crate::database::test_server::{local, real, serve};
use crate::database::{Client, Engine, Outcome};
use std::net::TcpStream;

const SCRAMBLE: [u8; 20] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
const OFFERED: u32 = LONG_PASSWORD | LONG_FLAG | CONNECT_WITH_DB | PROTOCOL_41 | TRANSACTIONS | SECURE_CONNECTION | MULTI_STATEMENTS | MULTI_RESULTS | PLUGIN_AUTH | PLUGIN_AUTH_LENENC;

fn packet(sequence: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_le_bytes()[..3].to_vec();
    out.push(sequence);
    out.extend_from_slice(payload);
    out
}

fn read_packet(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).unwrap();
    let mut body = vec![0u8; usize::from(head[0]) | usize::from(head[1]) << 8 | usize::from(head[2]) << 16];
    stream.read_exact(&mut body).unwrap();
    (head[3], body)
}

fn greeting(plugin: &str) -> Vec<u8> {
    let mut out = vec![10];
    out.extend(b"5.5.5-11.8.10-MariaDB-log\0");
    out.extend(77u32.to_le_bytes());
    out.extend(&SCRAMBLE[..8]);
    out.push(0);
    out.extend((OFFERED as u16).to_le_bytes());
    out.push(45);
    out.extend(2u16.to_le_bytes());
    out.extend(((OFFERED >> 16) as u16).to_le_bytes());
    out.push(21);
    out.extend([0; 10]);
    out.extend(&SCRAMBLE[8..]);
    out.push(0);
    out.extend(plugin.bytes().chain([0]));
    out
}

fn ok(rows: u8, status: u16) -> Vec<u8> {
    [&[0, rows, 0][..], &status.to_le_bytes(), &[0, 0]].concat()
}

fn eof(status: u16) -> Vec<u8> {
    [&[0xFE, 0, 0][..], &status.to_le_bytes()].concat()
}

fn lenenc_text(text: &[u8]) -> Vec<u8> {
    [&lenenc(text.len() as u64)[..], text].concat()
}

fn column(name: &str, kind: u8, charset: u16) -> Vec<u8> {
    let mut out = Vec::new();
    for part in ["def", "limtest", "t", "t", name, name] {
        out.extend(lenenc_text(part.as_bytes()));
    }
    out.push(0x0c);
    out.extend(charset.to_le_bytes());
    out.extend(255u32.to_le_bytes());
    out.push(kind);
    out.extend(0u16.to_le_bytes());
    out.extend([0, 0, 0]);
    out
}

/// Plays a login: the greeting, then reads the login packet and gives it back.
fn take_login(stream: &mut TcpStream, plugin: &str) -> Vec<u8> {
    stream.write_all(&packet(0, &greeting(plugin))).unwrap();
    let (sequence, login) = read_packet(stream);
    assert_eq!(sequence, 1);
    login
}

/// The answer in a login packet, after the user's name.
fn answer_in(login: &[u8]) -> Vec<u8> {
    let after_name = 32 + login[32..].iter().position(|b| *b == 0).unwrap() + 1;
    let length = usize::from(login[after_name]);
    login[after_name + 1..after_name + 1 + length].to_vec()
}

#[test]
fn the_password_answers_match_ones_worked_out_separately() {
    // Worked out with Python's hashlib.
    let hex = |bytes: Vec<u8>| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(hex(native_password("secret", &SCRAMBLE)), "b32bb3a583e1340c0a1108d58b1be49781ad8c2f");
    assert_eq!(hex(caching_sha2_password("secret", &SCRAMBLE)), "746ebe205d56a0707acb3e796e834e0dd7b1d61743b26bd5202c7a623230c7c9");
    assert!(native_password("", &SCRAMBLE).is_empty(), "no password, no answer");
}

#[test]
fn a_greeting_gives_the_version_thread_scramble_and_plugin() {
    let greeting = read_greeting(&greeting("caching_sha2_password")).unwrap();
    assert_eq!(greeting, Greeting { version: "11.8.10-MariaDB-log".into(), thread: 77, capabilities: OFFERED, scramble: SCRAMBLE.to_vec(), plugin: "caching_sha2_password".into() });
}

#[test]
fn length_encoded_numbers_go_both_ways() {
    for number in [0, 250, 251, 65_535, 65_536, 16_777_215, 16_777_216, u64::MAX] {
        let bytes = lenenc(number);
        assert_eq!(Reader::new(&bytes).lenenc().unwrap(), Some(number), "{number}");
    }
    assert_eq!(Reader::new(&[0xFB]).lenenc().unwrap(), None, "NULL");
}

#[test]
fn a_syntax_errors_place_is_found_from_near_and_its_line() {
    let message = "ERROR 1064 (42000): You have an error in your SQL syntax; check the manual that corresponds to your MariaDB server version for the right syntax to use near 'FORM t' at line 2";
    let sql = "SELECT 'é';\nSELECT 1 FORM t";
    assert_eq!(error_position(message, sql), Some(21), "counted in characters");
    assert_eq!(error_position("ERROR 1064 (42000): … to use near '' at line 1", "SELECT"), Some(6), "at the end");
    assert_eq!(error_position("ERROR 1146 (42S02): Table 'x.y' doesn't exist", "SELECT * FROM y"), None);
}

#[test]
fn an_err_packet_gives_its_number_state_and_message() {
    let packet = [&[0xFF][..], &1146u16.to_le_bytes(), b"#42S02Table 'limtest.nope' doesn't exist"].concat();
    let problem = error_from(&packet);
    assert_eq!(problem.message, "ERROR 1146 (42S02): Table 'limtest.nope' doesn't exist");
    assert_eq!(problem.code.as_deref(), Some("1146"));
}

#[test]
fn types_become_column_kinds_with_binary_ones_told_apart_by_their_character_set() {
    assert_eq!(kind_of(3, false), ColumnKind::Integer);
    assert_eq!(kind_of(246, false), ColumnKind::Real);
    assert_eq!(kind_of(12, false), ColumnKind::DateTime);
    assert_eq!(kind_of(10, false), ColumnKind::Date);
    assert_eq!(kind_of(245, false), ColumnKind::Json);
    assert_eq!(kind_of(253, false), ColumnKind::Text);
    assert_eq!(kind_of(252, true), ColumnKind::Binary);
}

#[test]
fn a_pretend_server_logs_in_with_native_passwords_and_gives_several_result_sets() {
    let (port, server) = serve(|mut stream| {
        let login = take_login(&mut stream, "mysql_native_password");
        assert_eq!(u32::from_le_bytes(login[..4].try_into().unwrap()) & SSL, 0, "no TLS asked for");
        assert_eq!(&login[32..40], b"limtest\0");
        assert_eq!(answer_in(&login), native_password("secret", &SCRAMBLE));
        assert!(login.ends_with(b"limtest\0mysql_native_password\0"), "the database and plugin");
        stream.write_all(&packet(2, &ok(0, 2))).unwrap();
        // Two statements: a result set, then an UPDATE.
        let (sequence, query) = read_packet(&mut stream);
        assert_eq!((sequence, query.as_slice()), (0, b"\x03SELECT a; UPDATE b".as_slice()));
        let mut out = packet(1, &[4]);
        out.extend(packet(2, &column("id", 3, 63)));
        out.extend(packet(3, &column("name", 253, 45)));
        out.extend(packet(4, &column("data", 252, 63)));
        out.extend(packet(5, &column("price", 246, 63)));
        out.extend(packet(6, &eof(2)));
        out.extend(packet(7, &[lenenc_text(b"1"), lenenc_text("Zoë".as_bytes()), lenenc_text(&[0, 255]), lenenc_text(b"9.90")].concat()));
        out.extend(packet(8, &[lenenc_text(b"2"), vec![0xFB], vec![0xFB], lenenc_text(b"1.5")].concat()));
        out.extend(packet(9, &eof(MORE_RESULTS | 2)));
        out.extend(packet(10, &ok(3, 2)));
        stream.write_all(&out).unwrap();
        // A syntax error.
        read_packet(&mut stream);
        let err = [&[0xFF][..], &1064u16.to_le_bytes(), b"#42000You have an error in your SQL syntax; check the manual for the right syntax to use near 'FORM t' at line 1"].concat();
        stream.write_all(&packet(1, &err)).unwrap();
        // Changing database.
        let (_, init) = read_packet(&mut stream);
        stream.write_all(&packet(1, &ok(0, 2))).unwrap();
        // The goodbye.
        (init, read_packet(&mut stream).1)
    });
    let mut client = MySql::connect(&local(Engine::MySql, port), "secret").unwrap();
    assert_eq!((client.server(), client.thread), ("MariaDB 11.8.10".to_string(), 77));
    let reply = client.run("SELECT a; UPDATE b", 100).unwrap();
    let Outcome::Rows { table, .. } = &reply.outcomes[0] else { panic!("{reply:?}") };
    assert_eq!(table.rows[0], [Cell::Int(1), Cell::Text("Zoë".into()), Cell::Bytes(vec![0, 255]), Cell::Text("9.90".into())]);
    assert_eq!(table.rows[1], [Cell::Int(2), Cell::Null, Cell::Null, Cell::Real(1.5)]);
    assert_eq!(reply.outcomes[1], Outcome::Done { command: String::new(), rows: Some(3) });
    let problem = client.run("SELECT 1 FORM t", 100).unwrap_err();
    assert_eq!((problem.position, problem.code.as_deref()), (Some(9), Some("1064")));
    client.use_database("other").unwrap();
    drop(client);
    let (init, quit) = server.join().unwrap();
    assert_eq!((init.as_slice(), quit.as_slice()), (b"\x02other".as_slice(), [COM_QUIT].as_slice()));
}

#[test]
fn caching_sha2_password_logs_in_fast_and_says_tls_is_needed_for_the_full_login() {
    // From the server's cache: 0x01 0x03, then OK.
    let (port, server) = serve(|mut stream| {
        let login = take_login(&mut stream, "caching_sha2_password");
        stream.write_all(&packet(2, &[1, 3])).unwrap();
        stream.write_all(&packet(3, &ok(0, 2))).unwrap();
        answer_in(&login)
    });
    MySql::connect(&local(Engine::MySql, port), "secret").unwrap();
    assert_eq!(server.join().unwrap(), caching_sha2_password("secret", &SCRAMBLE));
    // Not in the cache: 0x01 0x04 asks for the whole password, which isn't sent without TLS.
    let (port, server) = serve(|mut stream| {
        take_login(&mut stream, "caching_sha2_password");
        stream.write_all(&packet(2, &[1, 4])).unwrap();
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
        rest
    });
    let problem = MySql::connect(&local(Engine::MySql, port), "secret").err().expect("refused");
    assert!(problem.message.contains("needs TLS"), "{}", problem.message);
    assert!(!server.join().unwrap().windows(6).any(|w| w == b"secret"), "the password isn't sent in the clear");
}

#[test]
fn a_switch_to_another_plugin_is_answered_with_its_new_scramble() {
    let (port, server) = serve(|mut stream| {
        take_login(&mut stream, "caching_sha2_password");
        let scramble: Vec<u8> = (100..120).collect();
        stream.write_all(&packet(2, &[&[0xFE][..], b"mysql_native_password\0", &scramble, &[0]].concat())).unwrap();
        let (sequence, answer) = read_packet(&mut stream);
        stream.write_all(&packet(sequence + 1, &ok(0, 2))).unwrap();
        (sequence, answer, scramble)
    });
    MySql::connect(&local(Engine::MySql, port), "secret").unwrap();
    let (sequence, answer, scramble) = server.join().unwrap();
    assert_eq!((sequence, answer), (3, native_password("secret", &scramble)));
}

#[test]
fn a_server_without_tls_is_refused_when_tls_is_required() {
    let (port, _server) = serve(|mut stream| {
        stream.write_all(&packet(0, &greeting("mysql_native_password"))).unwrap();
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
    });
    let connection = Connection { tls: TlsMode::Require, ..local(Engine::MySql, port) };
    assert!(MySql::connect(&connection, "").err().expect("refused").message.contains("doesn't offer TLS"));
}

// ---------- A real server ----------

#[test]
fn a_real_mysql_server_logs_in_over_tls_and_runs_sql() {
    let Some((connection, password)) = real("LIMONITER_TEST_MYSQL") else { return };
    for tls in [TlsMode::Require, TlsMode::Disable] {
        let mut client = MySql::connect(&Connection { tls, ..connection.clone() }, &password).unwrap();
        assert_eq!(client.stream.is_tls(), tls == TlsMode::Require);
        let reply = client.run("SELECT 1 AS a, 'x' AS b, CAST(1.50 AS DECIMAL(5,2)) AS c, DATE '2026-11-22' AS d, NULL AS e, X'01FF' AS f", 10).unwrap();
        let Outcome::Rows { table, .. } = &reply.outcomes[0] else { panic!() };
        assert_eq!(table.rows[0], [Cell::Int(1), Cell::Text("x".into()), Cell::Text("1.50".into()), Cell::Text("2026-11-22".into()), Cell::Null, Cell::Bytes(vec![1, 255])]);
        assert_eq!(table.columns[3].kind, ColumnKind::Date);
    }
    let mut client = MySql::connect(&connection, &password).unwrap();
    let reply = client.run("CREATE TEMPORARY TABLE lim_t (id int PRIMARY KEY, name text); INSERT INTO lim_t VALUES (1, 'a'), (2, 'b'); SELECT * FROM lim_t ORDER BY id; SELECT 2", 10).unwrap();
    assert_eq!(reply.outcomes.len(), 4, "{reply:?}");
    assert_eq!(reply.outcomes[1], Outcome::Done { command: String::new(), rows: Some(2) });
    // MariaDB takes FORM for the column's name, so the mistake is at lim_t.
    let problem = client.run("SELECT 1 FORM lim_t", 10).unwrap_err();
    assert_eq!(problem.position, Some(14), "{problem:?}");
    assert!(client.run("SELECT 1", 10).is_ok(), "still usable after an error");
    client.use_database("information_schema").unwrap();
    let Outcome::Rows { table, .. } = &client.run("SELECT DATABASE()", 1).unwrap().outcomes[0] else { panic!() };
    assert_eq!(table.rows[0][0], Cell::Text("information_schema".into()));
    assert!(client.use_database("no_such_database_here").is_err());
}

#[test]
fn a_real_mysql_server_lists_its_tables_and_keys() {
    let Some((connection, password)) = real("LIMONITER_TEST_MYSQL") else { return };
    let mut client = MySql::connect(&connection, &password).unwrap();
    client.run("DROP TABLE IF EXISTS lim_child, lim_parent; CREATE TABLE lim_parent (id int PRIMARY KEY, name varchar(20)) ENGINE=InnoDB; CREATE TABLE lim_child (id int PRIMARY KEY, parent int, FOREIGN KEY (parent) REFERENCES lim_parent(id)) ENGINE=InnoDB; CREATE OR REPLACE VIEW lim_view AS SELECT id FROM lim_parent", 0).unwrap();
    let schemas = client.catalogue().unwrap();
    client.run("DROP VIEW lim_view; DROP TABLE lim_child, lim_parent", 0).unwrap();
    assert_eq!(schemas[0].name, connection.database, "the connection's own database first");
    let tables = &schemas[0].tables;
    let child = tables.iter().find(|t| t.name == "lim_child").expect("lim_child");
    assert!(child.columns[0].primary);
    assert_eq!(child.columns[1].refers_to.as_deref(), Some(format!("{}.lim_parent(id)", connection.database).as_str()));
    assert_eq!(tables.iter().find(|t| t.name == "lim_parent").unwrap().columns[1].kind, "varchar(20)");
    assert!(tables.iter().find(|t| t.name == "lim_view").unwrap().view);
}

#[test]
fn a_real_mysql_query_is_stopped_by_kill_query() {
    let Some((connection, password)) = real("LIMONITER_TEST_MYSQL") else { return };
    let mut client = MySql::connect(&connection, &password).unwrap();
    let stopper = client.stopper();
    let began = std::time::Instant::now();
    let stopping = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        stopper.stop()
    });
    // A stopped SLEEP gives 1 rather than an error; either way it's over long before its 20 seconds.
    let _ = client.run("SELECT SLEEP(20)", 10);
    stopping.join().unwrap().unwrap();
    assert!(began.elapsed() < std::time::Duration::from_secs(10));
    assert!(client.run("SELECT 1", 10).is_ok(), "the connection carries on");
}
