//! Limoniter's own PostgreSQL client: the frontend/backend protocol, version 3.0, with the simple query protocol.
//! It logs in with SCRAM-SHA-256, MD5 or a clear-text password, as the server asks, over TLS when it's offered or
//! required, and stops a running query with a CancelRequest on a second connection, as psql's Ctrl+C does.

use super::tls::Stream;
use super::{Client, Connection, DbError, Gathering, Outcome, Reply, Schema, Stopper, TlsMode};
use crate::data_table::{Cell, ColumnKind, DataColumn};
use std::io::{Read, Write};

/// The protocol's version, 3.0, as the startup message gives it.
const PROTOCOL: i32 = 196_608;
/// The codes that take the place of a version for asking for TLS and for cancelling.
const SSL_REQUEST: i32 = 80_877_103;
const CANCEL_REQUEST: i32 = 80_877_102;

pub struct Postgres {
    stream: Stream,
    connection: Connection,
    process: u32,
    secret: u32,
    version: String,
}

// ---------- Messages ----------

/// A message from the server: its type letter and its body.
struct Message {
    kind: u8,
    body: Vec<u8>,
}

/// Reads the pieces of a message's body in turn.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], DbError> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.bytes.len()).ok_or_else(|| DbError::lost(crate::i18n::tr("The server sent a message Limoniter couldn't read.")))?;
        let piece = &self.bytes[self.at..end];
        self.at = end;
        Ok(piece)
    }

    fn u8(&mut self) -> Result<u8, DbError> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, DbError> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().expect("two bytes")))
    }

    fn i32(&mut self) -> Result<i32, DbError> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    /// A string ended by a zero byte.
    fn string(&mut self) -> Result<String, DbError> {
        let rest = &self.bytes[self.at..];
        let end = rest.iter().position(|b| *b == 0).ok_or_else(|| DbError::lost(crate::i18n::tr("The server sent a message Limoniter couldn't read.")))?;
        self.at += end + 1;
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    fn rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.at..];
        self.at = self.bytes.len();
        rest
    }
}

/// A message to the server: its type letter, then its length (counting itself) and body.
fn message(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend_from_slice(&(body.len() as i32 + 4).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// A message with no type letter, as the first ones are: startup, SSLRequest and CancelRequest.
fn untyped(body: &[u8]) -> Vec<u8> {
    let mut out = (body.len() as i32 + 4).to_be_bytes().to_vec();
    out.extend_from_slice(body);
    out
}

/// The startup message: the protocol's version and who's logging in to what.
pub(super) fn startup_message(user: &str, database: &str) -> Vec<u8> {
    let mut body = PROTOCOL.to_be_bytes().to_vec();
    for (name, value) in [("user", user), ("database", database), ("application_name", "Limoniter"), ("client_encoding", "UTF8")] {
        if !value.is_empty() {
            body.extend_from_slice(name.as_bytes());
            body.push(0);
            body.extend_from_slice(value.as_bytes());
            body.push(0);
        }
    }
    body.push(0);
    untyped(&body)
}

fn read_message(stream: &mut impl Read) -> Result<Message, DbError> {
    let mut head = [0u8; 5];
    stream.read_exact(&mut head).map_err(DbError::io)?;
    let length = i32::from_be_bytes([head[1], head[2], head[3], head[4]]);
    // No message the server sends is anywhere near a gigabyte; one that says so has been misread.
    if !(4..=1 << 30).contains(&length) {
        return Err(DbError::lost(crate::i18n::tr("The server sent a message Limoniter couldn't read.")));
    }
    let mut body = vec![0u8; length as usize - 4];
    stream.read_exact(&mut body).map_err(DbError::io)?;
    Ok(Message { kind: head[0], body })
}

fn send(stream: &mut impl Write, bytes: &[u8]) -> Result<(), DbError> {
    stream.write_all(bytes).and_then(|()| stream.flush()).map_err(DbError::io)
}

/// An ErrorResponse or NoticeResponse's fields, by their code letters.
fn fields(body: &[u8]) -> Vec<(u8, String)> {
    let mut reader = Reader::new(body);
    let mut out = Vec::new();
    while let Ok(code) = reader.u8() {
        if code == 0 {
            break;
        }
        match reader.string() {
            Ok(value) => out.push((code, value)),
            Err(_) => break,
        }
    }
    out
}

/// An ErrorResponse as an error: "ERROR: message", with the detail and hint on lines of their own, and the place
/// in the SQL it points at (the server counts characters from 1).
pub(super) fn error_from(body: &[u8]) -> DbError {
    let fields = fields(body);
    let field = |code: u8| fields.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
    let severity = field(b'V').or_else(|| field(b'S')).unwrap_or_else(|| "ERROR".into());
    let mut message = format!("{severity}: {}", field(b'M').unwrap_or_default());
    for (code, label) in [(b'D', "DETAIL"), (b'H', "HINT")] {
        if let Some(more) = field(code) {
            message.push_str(&format!("\n{label}: {more}"));
        }
    }
    let position = field(b'P').and_then(|p| p.parse::<usize>().ok()).map(|p| p.saturating_sub(1));
    DbError { message, position, code: field(b'C'), ..DbError::default() }
}

fn notice_from(body: &[u8]) -> String {
    let fields = fields(body);
    let field = |code: u8| fields.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str());
    format!("{}: {}", field(b'V').or(field(b'S')).unwrap_or("NOTICE"), field(b'M').unwrap_or_default())
}

// ---------- Types ----------

/// What a column holds, from its type's OID (as pg_type lists them).
pub fn kind_of(oid: i32) -> ColumnKind {
    match oid {
        16 => ColumnKind::Bool,
        20 | 21 | 23 | 26 | 28 | 5069 => ColumnKind::Integer,
        700 | 701 | 790 | 1700 => ColumnKind::Real,
        1082 => ColumnKind::Date,
        1114 | 1184 => ColumnKind::DateTime,
        114 | 3802 => ColumnKind::Json,
        17 => ColumnKind::Binary,
        18 | 19 | 25 | 1042 | 1043 | 1083 | 1266 | 1186 | 2950 | 869 | 650 | 829 => ColumnKind::Text,
        _ => ColumnKind::Unknown,
    }
}

/// A value as text format sends it, made into a cell for its column.
fn cell_of(kind: ColumnKind, bytes: &[u8]) -> Cell {
    let text = String::from_utf8_lossy(bytes);
    match kind {
        // bytea's text form is \x and hexadecimal digits.
        ColumnKind::Binary => match text.strip_prefix("\\x").and_then(|hex| (0..hex.len() / 2).map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()).collect::<Option<Vec<u8>>>()) {
            Some(bytes) => Cell::Bytes(bytes),
            None => Cell::Text(text.into_owned()),
        },
        ColumnKind::Real if text == "Infinity" || text == "-Infinity" => Cell::Real(if text.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY }),
        _ => super::cell_from_text(kind, &text),
    }
}

fn row_description(body: &[u8]) -> Result<Vec<DataColumn>, DbError> {
    let mut reader = Reader::new(body);
    let count = reader.i16()?.max(0);
    let mut columns = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = reader.string()?;
        let _table = reader.i32()?;
        let _attribute = reader.i16()?;
        let oid = reader.i32()?;
        let _size = reader.i16()?;
        let _modifier = reader.i32()?;
        let _format = reader.i16()?;
        columns.push(DataColumn { name, kind: kind_of(oid) });
    }
    Ok(columns)
}

fn data_row(body: &[u8], columns: &[DataColumn]) -> Result<Vec<Cell>, DbError> {
    let mut reader = Reader::new(body);
    let count = reader.i16()?.max(0) as usize;
    let mut row = Vec::with_capacity(count);
    for i in 0..count {
        let length = reader.i32()?;
        if length < 0 {
            row.push(Cell::Null);
        } else {
            let kind = columns.get(i).map_or(ColumnKind::Unknown, |c| c.kind);
            row.push(cell_of(kind, reader.take(length as usize)?));
        }
    }
    Ok(row)
}

/// A CommandComplete's tag as what was done and how many rows: "INSERT 0 3" is an INSERT of 3 rows.
pub(super) fn done_from_tag(tag: &str) -> Outcome {
    let words: Vec<&str> = tag.split_whitespace().collect();
    let counted = matches!(words.first().copied(), Some("INSERT" | "UPDATE" | "DELETE" | "SELECT" | "MOVE" | "FETCH" | "COPY" | "MERGE"));
    match words.last().and_then(|n| n.parse::<u64>().ok()).filter(|_| counted) {
        Some(rows) => Outcome::Done { command: words[0].to_string(), rows: Some(rows) },
        None => Outcome::Done { command: tag.to_string(), rows: None },
    }
}

// ---------- Logging in ----------

/// SCRAM-SHA-256 (RFC 5802 and 7677), as PostgreSQL 10 and later ask for by default.
pub mod scram {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};

    fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    /// PBKDF2 with HMAC-SHA-256, for one block of 32 bytes, which is all SCRAM needs.
    fn salted_password(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
        let mut block = salt.to_vec();
        block.extend_from_slice(&1u32.to_be_bytes());
        let mut u = hmac(password, &block);
        let mut out = u;
        for _ in 1..iterations {
            u = hmac(password, &u);
            for (o, x) in out.iter_mut().zip(u) {
                *o ^= x;
            }
        }
        out
    }

    /// The client's first message, whole and without its "n,," header: "n=user,r=nonce".
    pub fn client_first(user: &str, nonce: &str) -> (String, String) {
        let bare = format!("n={},r={nonce}", user.replace('=', "=3D").replace(',', "=2C"));
        (format!("n,,{bare}"), bare)
    }

    /// The client's last message, from the server's first, and the signature the server's last should carry.
    pub fn client_final(password: &str, client_first_bare: &str, client_nonce: &str, server_first: &str) -> Result<(String, [u8; 32]), String> {
        let part = |key: &str| server_first.split(',').find_map(|p| p.strip_prefix(key)).map(str::to_string);
        let bad = || crate::i18n::tr("The server's SCRAM login wasn't as the standard has it.").to_string();
        let nonce = part("r=").ok_or_else(bad)?;
        if !nonce.starts_with(client_nonce) {
            return Err(bad());
        }
        let salt = crate::conversions::base64_decode(&part("s=").ok_or_else(bad)?).map_err(|_| bad())?;
        let iterations: u32 = part("i=").and_then(|i| i.parse().ok()).filter(|i| *i > 0).ok_or_else(bad)?;
        let salted = salted_password(password.as_bytes(), &salt, iterations);
        let client_key = hmac(&salted, b"Client Key");
        let stored_key: [u8; 32] = Sha256::digest(client_key).into();
        // "biws" is "n,," in base64: no channel binding.
        let without_proof = format!("c=biws,r={nonce}");
        let auth_message = format!("{client_first_bare},{server_first},{without_proof}");
        let signature = hmac(&stored_key, auth_message.as_bytes());
        let proof: Vec<u8> = client_key.iter().zip(signature).map(|(k, s)| k ^ s).collect();
        let server_key = hmac(&salted, b"Server Key");
        let server_signature = hmac(&server_key, auth_message.as_bytes());
        Ok((format!("{without_proof},p={}", crate::conversions::base64_encode(&proof, false)), server_signature))
    }

    /// Whether the server's last message proves it knew the password too.
    pub fn server_final_is_right(server_final: &str, expected: &[u8; 32]) -> bool {
        server_final.split(',').find_map(|p| p.strip_prefix("v=")).and_then(|v| crate::conversions::base64_decode(v).ok()).is_some_and(|v| v == expected)
    }
}

/// MD5's answer: "md5" and md5(md5(password + user) + salt) in hexadecimal.
pub fn md5_answer(user: &str, password: &str, salt: &[u8]) -> String {
    let inner = super::md5::hex(format!("{password}{user}").as_bytes());
    let mut outer = inner.into_bytes();
    outer.extend_from_slice(salt);
    format!("md5{}", super::md5::hex(&outer))
}

/// A random nonce for SCRAM, in printable characters.
fn nonce() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 18];
    let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
    crate::conversions::base64_encode(&bytes, false)
}

/// Opens the TCP connection and asks for TLS, as the connection says to.
fn open(connection: &Connection) -> Result<Stream, DbError> {
    let mut stream = Stream::connect(&connection.host, connection.port).map_err(DbError::lost)?;
    if connection.tls == TlsMode::Disable {
        return Ok(stream);
    }
    send(&mut stream, &untyped(&SSL_REQUEST.to_be_bytes()))?;
    let mut answer = [0u8; 1];
    stream.read_exact(&mut answer).map_err(DbError::io)?;
    match answer[0] {
        b'S' => stream.start_tls(&connection.host, connection.trust_own_certificate).map_err(DbError::lost),
        _ if connection.tls == TlsMode::Require => Err(DbError::lost(crate::i18n::tf!("{} doesn't offer TLS, and this connection is set always to use it.", connection.host))),
        b'N' => Ok(stream),
        _ => Err(DbError::lost(crate::i18n::tr("The server sent a message Limoniter couldn't read."))),
    }
}

impl Postgres {
    pub fn connect(connection: &Connection, password: &str) -> Result<Postgres, DbError> {
        let mut stream = open(connection)?;
        let database = if connection.database.is_empty() { &connection.user } else { &connection.database };
        send(&mut stream, &startup_message(&connection.user, database))?;
        let mut scram: Option<(String, String, [u8; 32])> = None;
        let (mut process, mut secret, mut version) = (0, 0, String::new());
        loop {
            let reply = read_message(&mut stream)?;
            match reply.kind {
                b'R' => {
                    let mut reader = Reader::new(&reply.body);
                    match reader.i32()? {
                        0 => {}
                        3 => {
                            let mut body = password.as_bytes().to_vec();
                            body.push(0);
                            send(&mut stream, &message(b'p', &body))?;
                        }
                        5 => {
                            let salt = reader.take(4)?;
                            let mut body = md5_answer(&connection.user, password, salt).into_bytes();
                            body.push(0);
                            send(&mut stream, &message(b'p', &body))?;
                        }
                        10 => {
                            let mut offered = Vec::new();
                            while let Ok(name) = reader.string() {
                                if name.is_empty() {
                                    break;
                                }
                                offered.push(name);
                            }
                            if !offered.iter().any(|m| m == "SCRAM-SHA-256") {
                                return Err(DbError::lost(crate::i18n::tf!("The server asks for a login Limoniter doesn't know: {}.", offered.join(", "))));
                            }
                            let client_nonce = nonce();
                            // PostgreSQL takes the user from the startup message and leaves this one empty.
                            let (first, bare) = scram::client_first("", &client_nonce);
                            let mut body = b"SCRAM-SHA-256\0".to_vec();
                            body.extend_from_slice(&(first.len() as i32).to_be_bytes());
                            body.extend_from_slice(first.as_bytes());
                            send(&mut stream, &message(b'p', &body))?;
                            scram = Some((bare, client_nonce, [0; 32]));
                        }
                        11 => {
                            let Some((bare, client_nonce, _)) = scram.as_ref() else {
                                return Err(DbError::lost(crate::i18n::tr("The server's SCRAM login wasn't as the standard has it.")));
                            };
                            let server_first = String::from_utf8_lossy(reader.rest()).into_owned();
                            let (last, signature) = scram::client_final(password, bare, client_nonce, &server_first).map_err(DbError::lost)?;
                            send(&mut stream, &message(b'p', last.as_bytes()))?;
                            scram = scram.map(|(b, n, _)| (b, n, signature));
                        }
                        12 => {
                            let server_final = String::from_utf8_lossy(reader.rest()).into_owned();
                            if !scram.as_ref().is_some_and(|(_, _, signature)| scram::server_final_is_right(&server_final, signature)) {
                                return Err(DbError::lost(crate::i18n::tr("The server couldn't prove it knew the password, so Limoniter didn't trust it.")));
                            }
                        }
                        other => return Err(DbError::lost(crate::i18n::tf!("The server asks for a kind of login Limoniter doesn't know (number {other})."))),
                    }
                }
                b'K' => {
                    let mut reader = Reader::new(&reply.body);
                    process = reader.i32()? as u32;
                    secret = reader.i32()? as u32;
                }
                b'S' => {
                    let mut reader = Reader::new(&reply.body);
                    if reader.string()? == "server_version" {
                        version = reader.string()?;
                    }
                }
                b'E' => {
                    let mut problem = error_from(&reply.body);
                    problem.lost = true;
                    return Err(problem);
                }
                b'N' => {}
                b'Z' => break,
                _ => {}
            }
        }
        Ok(Postgres { stream, connection: connection.clone(), process, secret, version })
    }
}

impl Client for Postgres {
    fn run(&mut self, sql: &str, most: usize) -> Result<Reply, DbError> {
        let mut body = sql.as_bytes().to_vec();
        body.push(0);
        send(&mut self.stream, &message(b'Q', &body))?;
        let mut reply = Reply::default();
        let mut gathering: Option<Gathering> = None;
        let mut problem: Option<DbError> = None;
        loop {
            let got = read_message(&mut self.stream)?;
            match got.kind {
                b'T' => gathering = Some(Gathering::new(row_description(&got.body)?, most)),
                b'D' => {
                    if let Some(rows) = gathering.as_mut() {
                        // Rows past the most kept are only counted, not read.
                        if rows.wants_more() {
                            let row = data_row(&got.body, &rows.table.columns)?;
                            rows.push(row);
                        } else {
                            rows.more += 1;
                        }
                    }
                }
                b'C' => {
                    let tag = Reader::new(&got.body).string()?;
                    match gathering.take() {
                        Some(rows) => reply.outcomes.push(rows.finish()),
                        None => reply.outcomes.push(done_from_tag(&tag)),
                    }
                }
                b'I' => {}
                b'E' => {
                    gathering = None;
                    problem = Some(error_from(&got.body));
                }
                b'N' => reply.notices.push(notice_from(&got.body)),
                // COPY FROM STDIN: there's nothing to send, so it's refused.
                b'G' => send(&mut self.stream, &message(b'f', b"COPY FROM STDIN isn't something Limoniter can send.\0"))?,
                b'H' => reply.notices.push(crate::i18n::tr("COPY TO STDOUT's rows aren't shown; use SELECT to see them.").into()),
                b'Z' => break,
                // ParameterStatus, NotificationResponse, CopyData and CopyDone need nothing.
                _ => {}
            }
        }
        match problem {
            Some(mut problem) => {
                problem.before = reply;
                Err(problem)
            }
            None => Ok(reply),
        }
    }

    fn catalogue(&mut self) -> Result<Vec<Schema>, DbError> {
        const TABLES: &str = "SELECT n.nspname, c.relname, c.relkind IN ('v', 'm'), coalesce(a.attname, ''), coalesce(format_type(a.atttypid, a.atttypmod), '') \
            FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
            LEFT JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
            WHERE c.relkind IN ('r', 'v', 'm', 'p', 'f') AND n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%' \
            ORDER BY n.nspname = 'public' DESC, n.nspname, c.relname, a.attnum";
        const KEYS: &str = "SELECT n.nspname, c.relname, a.attname, co.contype::text, coalesce(fn.nspname || '.' || fc.relname || '(' || fa.attname || ')', '') \
            FROM pg_constraint co JOIN pg_class c ON c.oid = co.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
            JOIN LATERAL unnest(co.conkey) WITH ORDINALITY AS k(attnum, ord) ON true \
            JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
            LEFT JOIN pg_class fc ON fc.oid = co.confrelid LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
            LEFT JOIN pg_attribute fa ON fa.attrelid = co.confrelid AND fa.attnum = co.confkey[k.ord] \
            WHERE co.contype IN ('p', 'f')";
        let rows = super::rows_of(self.run(TABLES, usize::MAX)?);
        let mut schemas = super::catalogue_from_rows(rows.iter().map(|r| (super::text_of(r.first()), super::text_of(r.get(1)), r.get(2) == Some(&Cell::Bool(true)), super::text_of(r.get(3)), super::text_of(r.get(4)))));
        let keys = super::rows_of(self.run(KEYS, usize::MAX)?);
        let key = |r: &Vec<Cell>| (super::text_of(r.first()), super::text_of(r.get(1)), super::text_of(r.get(2)));
        let primary: Vec<_> = keys.iter().filter(|r| super::text_of(r.get(3)) == "p").map(key).collect();
        let foreign: Vec<_> = keys.iter().filter(|r| super::text_of(r.get(3)) == "f").map(|r| { let (s, t, c) = key(r); (s, t, c, super::text_of(r.get(4))) }).collect();
        super::mark_keys(&mut schemas, &primary, &foreign);
        Ok(schemas)
    }

    fn stopper(&self) -> Stopper {
        Stopper::Postgres { connection: self.connection.clone(), process: self.process, secret: self.secret }
    }

    fn server(&self) -> String {
        format!("PostgreSQL {}", self.version)
    }
}

impl Drop for Postgres {
    fn drop(&mut self) {
        // Terminate, so the server doesn't log the connection as lost.
        let _ = send(&mut self.stream, &message(b'X', &[]));
    }
}

/// The CancelRequest: on a connection of its own, the process and secret the server gave when logging in.
pub(super) fn cancel_message(process: u32, secret: u32) -> Vec<u8> {
    let mut body = CANCEL_REQUEST.to_be_bytes().to_vec();
    body.extend_from_slice(&process.to_be_bytes());
    body.extend_from_slice(&secret.to_be_bytes());
    untyped(&body)
}

/// Asks the server to stop the query running on another connection.
pub fn cancel(connection: &Connection, process: u32, secret: u32) -> Result<(), String> {
    let mut stream = open(connection).map_err(|e| e.message)?;
    send(&mut stream, &cancel_message(process, secret)).map_err(|e| e.message)?;
    // The server closes the connection once it's read the request; waiting for that makes sure it was.
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest);
    Ok(())
}

#[cfg(test)]
#[path = "postgres_tests.rs"]
mod tests;
