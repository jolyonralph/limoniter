use super::*;
use crate::database::test_server::{local, real, serve};
use crate::database::{Engine, Outcome};
use std::net::TcpStream;

fn int32(n: i32) -> [u8; 4] {
    n.to_be_bytes()
}

/// Reads a typed message from the client.
fn read_typed(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let got = read_message(stream).expect("a message from the client");
    (got.kind, got.body)
}

/// Reads an untyped one (startup, SSLRequest, CancelRequest).
fn read_untyped(stream: &mut TcpStream) -> Vec<u8> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).unwrap();
    let mut body = vec![0u8; i32::from_be_bytes(length) as usize - 4];
    stream.read_exact(&mut body).unwrap();
    body
}

fn string_body(parts: &[&str]) -> Vec<u8> {
    parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
}

/// What a server says once a login's accepted: the key for cancelling, the server's version, and ready.
fn logged_in(stream: &mut TcpStream) {
    let mut out = message(b'R', &int32(0));
    out.extend(message(b'K', &[int32(4242), int32(99)].concat()));
    out.extend(message(b'S', &string_body(&["server_version", "14.99"])));
    out.extend(message(b'Z', b"I"));
    stream.write_all(&out).unwrap();
}

fn row_description_of(columns: &[(&str, i32)]) -> Vec<u8> {
    let mut body = (columns.len() as i16).to_be_bytes().to_vec();
    for (name, oid) in columns {
        body.extend(name.bytes().chain([0]));
        body.extend(int32(0));
        body.extend(0i16.to_be_bytes());
        body.extend(int32(*oid));
        body.extend((-1i16).to_be_bytes());
        body.extend(int32(-1));
        body.extend(0i16.to_be_bytes());
    }
    message(b'T', &body)
}

fn data_row_of(values: &[Option<&str>]) -> Vec<u8> {
    let mut body = (values.len() as i16).to_be_bytes().to_vec();
    for value in values {
        match value {
            Some(v) => {
                body.extend(int32(v.len() as i32));
                body.extend(v.as_bytes());
            }
            None => body.extend(int32(-1)),
        }
    }
    message(b'D', &body)
}

#[test]
fn scram_sha_256_gives_rfc_7677s_proof_and_checks_the_servers() {
    // RFC 7677, section 3.
    let (first, bare) = scram::client_first("user", "rOprNGfwEbeRWgbNEkqO");
    assert_eq!(first, "n,,n=user,r=rOprNGfwEbeRWgbNEkqO");
    let server_first = "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
    let (last, signature) = scram::client_final("pencil", &bare, "rOprNGfwEbeRWgbNEkqO", server_first).unwrap();
    assert_eq!(last, "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=");
    assert!(scram::server_final_is_right("v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=", &signature));
    assert!(!scram::server_final_is_right("v=AAAATRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=", &signature));
    // A server's nonce that doesn't start with the client's is refused.
    assert!(scram::client_final("pencil", &bare, "other", server_first).is_err());
}

#[test]
fn md5s_answer_is_md5_of_md5_of_password_and_user_then_the_salt() {
    // Worked out separately with Python's hashlib.
    assert_eq!(md5_answer("limtest", "secret", &[1, 2, 3, 4]), "md57b67b23a8a547fb4619ac2fc481b21b7");
}

#[test]
fn startup_and_cancel_messages_are_as_the_protocol_has_them() {
    let startup = startup_message("me", "shop");
    assert_eq!(&startup[..8], &[0, 0, 0, startup.len() as u8, 0, 3, 0, 0]);
    assert!(startup.ends_with(b"client_encoding\0UTF8\0\0"));
    assert_eq!(cancel_message(0x0102_0304, 7), [0, 0, 0, 16, 0x04, 0xD2, 0x16, 0x2E, 1, 2, 3, 4, 0, 0, 0, 7]);
}

#[test]
fn an_error_response_gives_its_message_detail_and_place() {
    let body = [b"SERROR\0".as_slice(), b"VERROR\0", b"C42601\0", b"Msyntax error at or near \"FORM\"\0", b"HCheck the spelling.\0", b"P10\0", b"\0"].concat();
    let problem = error_from(&body);
    assert_eq!(problem.message, "ERROR: syntax error at or near \"FORM\"\nHINT: Check the spelling.");
    assert_eq!(problem.position, Some(9), "the server counts from 1");
    assert_eq!(problem.code.as_deref(), Some("42601"));
}

#[test]
fn command_tags_say_what_was_done_and_to_how_many_rows() {
    assert_eq!(done_from_tag("INSERT 0 3"), Outcome::Done { command: "INSERT".into(), rows: Some(3) });
    assert_eq!(done_from_tag("UPDATE 12"), Outcome::Done { command: "UPDATE".into(), rows: Some(12) });
    assert_eq!(done_from_tag("CREATE TABLE"), Outcome::Done { command: "CREATE TABLE".into(), rows: None });
}

#[test]
fn types_by_oid_become_column_kinds() {
    assert_eq!([16, 20, 23, 701, 1700, 1082, 1184, 3802, 17, 1043, 600].map(kind_of), [
        ColumnKind::Bool,
        ColumnKind::Integer,
        ColumnKind::Integer,
        ColumnKind::Real,
        ColumnKind::Real,
        ColumnKind::Date,
        ColumnKind::DateTime,
        ColumnKind::Json,
        ColumnKind::Binary,
        ColumnKind::Text,
        ColumnKind::Unknown
    ]);
}

#[test]
fn a_pretend_server_logs_in_with_md5_and_answers_queries() {
    let (port, server) = serve(|mut stream| {
        let startup = read_untyped(&mut stream);
        assert!(startup.windows(8).any(|w| w == b"limtest\0"));
        stream.write_all(&message(b'R', &[int32(5).as_slice(), &[1, 2, 3, 4]].concat())).unwrap();
        let (kind, body) = read_typed(&mut stream);
        assert_eq!((kind, body.as_slice()), (b'p', b"md57b67b23a8a547fb4619ac2fc481b21b7\0".as_slice()));
        logged_in(&mut stream);
        // A query with rows of every kind, then a notice.
        let (kind, body) = read_typed(&mut stream);
        assert_eq!((kind, body.as_slice()), (b'Q', b"SELECT everything\0".as_slice()));
        let mut out = row_description_of(&[("id", 23), ("name", 25), ("ok", 16), ("data", 17), ("price", 1700), ("when", 1082)]);
        out.extend(data_row_of(&[Some("1"), Some("Ann"), Some("t"), Some("\\x0aff"), Some("12.50"), Some("2026-11-22")]));
        out.extend(data_row_of(&[Some("2"), None, Some("f"), None, Some("3"), None]));
        out.extend(message(b'C', b"SELECT 2\0"));
        out.extend(message(b'N', b"SNOTICE\0Mjust so you know\0\0"));
        out.extend(message(b'Z', b"I"));
        stream.write_all(&out).unwrap();
        // Two statements: the first works, the second fails at its 8th character.
        read_typed(&mut stream);
        let mut out = message(b'C', b"INSERT 0 2\0");
        out.extend(message(b'E', b"SERROR\0C42601\0Msyntax error\0P8\0\0"));
        out.extend(message(b'Z', b"E"));
        stream.write_all(&out).unwrap();
        // Rows past the most kept are counted.
        read_typed(&mut stream);
        let mut out = row_description_of(&[("n", 20)]);
        for n in 0..5 {
            out.extend(data_row_of(&[Some(&n.to_string())]));
        }
        out.extend(message(b'C', b"SELECT 5\0"));
        out.extend(message(b'Z', b"I"));
        stream.write_all(&out).unwrap();
        // The goodbye.
        read_typed(&mut stream).0
    });
    let mut client = Postgres::connect(&local(Engine::Postgres, port), "secret").unwrap();
    assert_eq!((client.server(), client.process, client.secret), ("PostgreSQL 14.99".to_string(), 4242, 99));
    let reply = client.run("SELECT everything", 100).unwrap();
    let Outcome::Rows { table, more: 0 } = &reply.outcomes[0] else { panic!("rows, not {:?}", reply.outcomes) };
    assert_eq!(table.columns.iter().map(|c| c.kind).collect::<Vec<_>>(), [ColumnKind::Integer, ColumnKind::Text, ColumnKind::Bool, ColumnKind::Binary, ColumnKind::Real, ColumnKind::Date]);
    assert_eq!(table.rows[0], [Cell::Int(1), Cell::Text("Ann".into()), Cell::Bool(true), Cell::Bytes(vec![0x0a, 0xff]), Cell::Text("12.50".into()), Cell::Text("2026-11-22".into())]);
    assert_eq!(table.rows[1], [Cell::Int(2), Cell::Null, Cell::Bool(false), Cell::Null, Cell::Real(3.0), Cell::Null]);
    assert_eq!(reply.notices, ["NOTICE: just so you know"]);
    let problem = client.run("INSERT …; SELEC 1", 100).unwrap_err();
    assert_eq!((problem.position, problem.lost), (Some(7), false));
    assert_eq!(problem.before.outcomes, [Outcome::Done { command: "INSERT".into(), rows: Some(2) }], "what worked before it is kept");
    let Outcome::Rows { table, more } = &client.run("SELECT n", 3).unwrap().outcomes[0] else { panic!() };
    assert_eq!((table.rows.len(), *more), (3, 2));
    drop(client);
    assert_eq!(server.join().unwrap(), b'X', "it says goodbye");
}

#[test]
fn a_pretend_server_takes_a_clear_text_password_and_refuses_a_wrong_one() {
    let (port, server) = serve(|mut stream| {
        read_untyped(&mut stream);
        stream.write_all(&message(b'R', &int32(3))).unwrap();
        let (_, body) = read_typed(&mut stream);
        stream.write_all(&message(b'E', b"SFATAL\0C28P01\0Mpassword authentication failed for user \"limtest\"\0\0")).unwrap();
        body
    });
    let problem = Postgres::connect(&local(Engine::Postgres, port), "wrong").err().expect("refused");
    assert_eq!(server.join().unwrap(), b"wrong\0");
    assert!(problem.lost);
    assert_eq!(problem.message, "FATAL: password authentication failed for user \"limtest\"");
}

#[test]
fn a_server_without_tls_is_refused_when_tls_is_required_and_used_plain_when_preferred() {
    for tls in [TlsMode::Require, TlsMode::Prefer] {
        let (port, server) = serve(move |mut stream| {
            let request = read_untyped(&mut stream);
            stream.write_all(b"N").unwrap();
            // When TLS is only preferred, the startup comes next, in the clear.
            if tls == TlsMode::Prefer {
                read_untyped(&mut stream);
                stream.write_all(&[message(b'R', &int32(0)), message(b'Z', b"I")].concat()).unwrap();
            }
            request
        });
        let connection = Connection { tls, ..local(Engine::Postgres, port) };
        let result = Postgres::connect(&connection, "");
        match tls {
            TlsMode::Require => assert!(result.err().expect("refused").message.contains("doesn't offer TLS")),
            _ => drop(result.expect("connected in the clear")),
        }
        assert_eq!(server.join().unwrap(), [0x04, 0xD2, 0x16, 0x2F], "an SSLRequest");
    }
}

#[test]
fn stopping_sends_a_cancel_request_on_a_new_connection() {
    let (port, server) = serve(|mut stream| read_untyped(&mut stream));
    cancel(&local(Engine::Postgres, port), 4242, 99).unwrap();
    assert_eq!(server.join().unwrap(), [[0x04, 0xD2, 0x16, 0x2E], int32(4242), int32(99)].concat());
}

// ---------- A real server ----------

#[test]
fn a_real_postgresql_server_logs_in_with_scram_over_tls_and_runs_sql() {
    let Some((connection, password)) = real("LIMONITER_TEST_POSTGRES") else { return };
    for tls in [TlsMode::Require, TlsMode::Disable] {
        let mut client = Postgres::connect(&Connection { tls, ..connection.clone() }, &password).unwrap();
        assert_eq!(client.stream.is_tls(), tls == TlsMode::Require);
        assert!(client.server().starts_with("PostgreSQL "));
        let reply = client.run("SELECT 1::int4 AS a, 'x'::text AS b, true AS c, '\\x01ff'::bytea AS d, 1.50::numeric AS e, DATE '2026-11-22' AS f, '{\"k\": 1}'::jsonb AS g, NULL::int AS h", 10).unwrap();
        let Outcome::Rows { table, .. } = &reply.outcomes[0] else { panic!() };
        assert_eq!(table.rows[0], [Cell::Int(1), Cell::Text("x".into()), Cell::Bool(true), Cell::Bytes(vec![1, 255]), Cell::Text("1.50".into()), Cell::Text("2026-11-22".into()), Cell::Text("{\"k\": 1}".into()), Cell::Null]);
        assert_eq!(table.columns[6].kind, ColumnKind::Json);
    }
    let mut client = Postgres::connect(&connection, &password).unwrap();
    let reply = client.run("CREATE TEMP TABLE lim_t (id serial PRIMARY KEY, name text); INSERT INTO lim_t (name) VALUES ('a'), ('b'); SELECT * FROM lim_t ORDER BY id", 10).unwrap();
    assert_eq!(reply.outcomes.len(), 3);
    assert_eq!(reply.outcomes[1], Outcome::Done { command: "INSERT".into(), rows: Some(2) });
    // PostgreSQL takes FORM for the column's name, so the mistake is at lim_t.
    let problem = client.run("SELECT 1 FORM lim_t", 10).unwrap_err();
    assert_eq!(problem.position, Some(14), "{problem:?}");
    assert!(!problem.lost);
    // The connection still works after an error.
    assert!(client.run("SELECT 1", 10).is_ok());
}

#[test]
fn a_real_postgresql_server_lists_its_tables_and_keys() {
    let Some((connection, password)) = real("LIMONITER_TEST_POSTGRES") else { return };
    let mut client = Postgres::connect(&connection, &password).unwrap();
    client.run("DROP TABLE IF EXISTS lim_child, lim_parent; CREATE TABLE lim_parent (id int PRIMARY KEY, name varchar(20)); CREATE TABLE lim_child (id int PRIMARY KEY, parent int REFERENCES lim_parent(id))", 0).unwrap();
    let schemas = client.catalogue().unwrap();
    client.run("DROP TABLE lim_child, lim_parent", 0).unwrap();
    let public = schemas.iter().find(|s| s.name == "public").expect("public");
    let child = public.tables.iter().find(|t| t.name == "lim_child").expect("lim_child");
    assert!(child.columns[0].primary);
    assert_eq!(child.columns[1].refers_to.as_deref(), Some("public.lim_parent(id)"));
    let parent = public.tables.iter().find(|t| t.name == "lim_parent").unwrap();
    assert_eq!(parent.columns[1].kind, "character varying(20)");
}

#[test]
fn a_real_postgresql_query_is_stopped_by_a_cancel_request() {
    let Some((connection, password)) = real("LIMONITER_TEST_POSTGRES") else { return };
    let mut client = Postgres::connect(&connection, &password).unwrap();
    let stopper = client.stopper();
    let began = std::time::Instant::now();
    let stopping = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        stopper.stop()
    });
    let problem = client.run("SELECT pg_sleep(20)", 10).unwrap_err();
    stopping.join().unwrap().unwrap();
    assert_eq!(problem.code.as_deref(), Some("57014"), "query_canceled: {problem:?}");
    assert!(began.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn a_real_postgresql_server_refuses_a_wrong_password() {
    let Some((connection, _)) = real("LIMONITER_TEST_POSTGRES") else { return };
    let problem = Postgres::connect(&connection, "not the password").err().expect("refused");
    assert_eq!(problem.code.as_deref(), Some("28P01"), "{problem:?}");
}

#[test]
fn a_real_servers_own_certificate_is_refused_unless_it_is_accepted() {
    let Some((connection, password)) = real("LIMONITER_TEST_POSTGRES") else { return };
    let checked = Connection { tls: TlsMode::Require, trust_own_certificate: false, ..connection };
    let problem = Postgres::connect(&checked, &password).err().expect("the test server's certificate is its own");
    assert!(problem.message.contains("certificate"), "{}", problem.message);
}
