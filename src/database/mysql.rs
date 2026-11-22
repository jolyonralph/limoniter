//! Limoniter's own MySQL and MariaDB client: the handshake (version 10), logging in with mysql_native_password or
//! caching_sha2_password, TLS through the SSLRequest packet, COM_QUERY's text result sets (several of them, when
//! several statements are sent), COM_INIT_DB to change database, and KILL QUERY from a second connection to stop
//! a query.

use super::tls::Stream;
use super::{Client, Connection, DbError, Gathering, Outcome, Reply, Schema, Stopper, TlsMode};
use crate::data_table::{Cell, ColumnKind, DataColumn};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

// The capability flags Limoniter asks for.
const LONG_PASSWORD: u32 = 1;
const LONG_FLAG: u32 = 4;
const CONNECT_WITH_DB: u32 = 8;
const PROTOCOL_41: u32 = 0x200;
const SSL: u32 = 0x800;
const TRANSACTIONS: u32 = 0x2000;
const SECURE_CONNECTION: u32 = 0x8000;
const MULTI_STATEMENTS: u32 = 0x1_0000;
const MULTI_RESULTS: u32 = 0x2_0000;
const PLUGIN_AUTH: u32 = 0x8_0000;
const PLUGIN_AUTH_LENENC: u32 = 0x20_0000;

/// The status flag that says another result set follows.
const MORE_RESULTS: u16 = 0x0008;
/// utf8mb4_general_ci, which MySQL and MariaDB both have.
const UTF8MB4: u8 = 45;
/// The character set that marks a column's bytes as binary rather than text.
const BINARY: u16 = 63;
const MAX_PACKET: u32 = 16 * 1024 * 1024;

const COM_QUIT: u8 = 0x01;
const COM_INIT_DB: u8 = 0x02;
const COM_QUERY: u8 = 0x03;

pub struct MySql {
    stream: Stream,
    /// The next packet's sequence number.
    sequence: u8,
    connection: Connection,
    password: String,
    thread: u32,
    version: String,
}

// ---------- Packets ----------

/// Reads the pieces of a packet in turn.
pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

fn unreadable() -> DbError {
    DbError::lost(crate::i18n::tr("The server sent a message Limoniter couldn't read."))
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], DbError> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.bytes.len()).ok_or_else(unreadable)?;
        let piece = &self.bytes[self.at..end];
        self.at = end;
        Ok(piece)
    }

    fn u8(&mut self) -> Result<u8, DbError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, DbError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("two bytes")))
    }

    fn u32(&mut self) -> Result<u32, DbError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    /// A length-encoded number; None for 0xFB, which is NULL in a row.
    fn lenenc(&mut self) -> Result<Option<u64>, DbError> {
        Ok(Some(match self.u8()? {
            0xFB => return Ok(None),
            0xFC => u64::from(self.u16()?),
            0xFD => {
                let b = self.take(3)?;
                u64::from(b[0]) | u64::from(b[1]) << 8 | u64::from(b[2]) << 16
            }
            0xFE => u64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")),
            n => u64::from(n),
        }))
    }

    fn lenenc_bytes(&mut self) -> Result<Option<&'a [u8]>, DbError> {
        match self.lenenc()? {
            Some(length) => Ok(Some(self.take(usize::try_from(length).map_err(|_| unreadable())?)?)),
            None => Ok(None),
        }
    }

    fn lenenc_string(&mut self) -> Result<String, DbError> {
        Ok(String::from_utf8_lossy(self.lenenc_bytes()?.unwrap_or_default()).into_owned())
    }

    /// A string ended by a zero byte, or by the end of the packet.
    fn string(&mut self) -> Result<&'a [u8], DbError> {
        let rest = &self.bytes[self.at..];
        let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        self.at += (end + 1).min(rest.len());
        Ok(&rest[..end])
    }

    fn rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.at..];
        self.at = self.bytes.len();
        rest
    }

    fn at_end(&self) -> bool {
        self.at >= self.bytes.len()
    }
}

fn lenenc(number: u64) -> Vec<u8> {
    match number {
        0..=0xFA => vec![number as u8],
        0xFB..=0xFFFF => [&[0xFC][..], &(number as u16).to_le_bytes()].concat(),
        0x1_0000..=0xFF_FFFF => [&[0xFD][..], &(number as u32).to_le_bytes()[..3]].concat(),
        _ => [&[0xFE][..], &number.to_le_bytes()].concat(),
    }
}

/// The server's greeting: handshake version 10.
#[derive(Debug, PartialEq)]
pub(super) struct Greeting {
    pub version: String,
    pub thread: u32,
    pub capabilities: u32,
    /// The scramble the password is mixed with: 20 bytes.
    pub scramble: Vec<u8>,
    pub plugin: String,
}

pub(super) fn read_greeting(packet: &[u8]) -> Result<Greeting, DbError> {
    let mut reader = Reader::new(packet);
    if packet.first() == Some(&0xFF) {
        return Err(error_from(packet));
    }
    if reader.u8()? != 10 {
        return Err(DbError::lost(crate::i18n::tr("The server speaks a version of MySQL's protocol older than Limoniter knows.")));
    }
    let version = String::from_utf8_lossy(reader.string()?).into_owned();
    let thread = reader.u32()?;
    let mut scramble = reader.take(8)?.to_vec();
    reader.u8()?;
    let mut capabilities = u32::from(reader.u16()?);
    let mut plugin = String::new();
    if !reader.at_end() {
        let _charset = reader.u8()?;
        let _status = reader.u16()?;
        capabilities |= u32::from(reader.u16()?) << 16;
        let data_length = reader.u8()?;
        reader.take(10)?;
        if capabilities & SECURE_CONNECTION != 0 {
            let more = (usize::from(data_length).saturating_sub(8)).max(13);
            let part = reader.take(more.min(packet.len() - reader.at))?;
            // The second part ends with a zero byte that isn't part of the scramble.
            scramble.extend_from_slice(part.strip_suffix(&[0]).unwrap_or(part));
        }
        if capabilities & PLUGIN_AUTH != 0 {
            plugin = String::from_utf8_lossy(reader.string()?).into_owned();
        }
    }
    // MariaDB's version starts "5.5.5-" for old clients' sake.
    let version = version.strip_prefix("5.5.5-").map_or(version.clone(), str::to_string);
    Ok(Greeting { version, thread, capabilities, scramble, plugin })
}

/// An ERR packet as an error, with the SQL's place when the message gives one.
pub(super) fn error_from(packet: &[u8]) -> DbError {
    let mut reader = Reader::new(packet);
    let _ = reader.u8();
    let number = reader.u16().unwrap_or(0);
    let mut rest = reader.rest();
    let mut state = String::new();
    if rest.first() == Some(&b'#') && rest.len() >= 6 {
        state = String::from_utf8_lossy(&rest[1..6]).into_owned();
        rest = &rest[6..];
    }
    let message = String::from_utf8_lossy(rest).into_owned();
    let words = if state.is_empty() { format!("ERROR {number}: {message}") } else { format!("ERROR {number} ({state}): {message}") };
    DbError { message: words, code: Some(number.to_string()), ..DbError::default() }
}

/// Where in the SQL MySQL's syntax error points: its messages end "near 'the rest' at line 3", and the rest is
/// found on that line or after it. A count of characters from the start of the SQL.
pub fn error_position(message: &str, sql: &str) -> Option<usize> {
    let near_at = message.rfind("near '")? + "near '".len();
    let tail = &message[near_at..];
    let (near, line) = tail.rsplit_once("' at line ")?;
    let line: usize = line.trim().parse().ok()?;
    let line_start: usize = sql.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum();
    // MySQL cuts what it quotes at 80 characters; the first part is enough to find it.
    let near: String = near.chars().take(40).collect();
    if near.is_empty() {
        return Some(sql.chars().count());
    }
    let found = sql.get(line_start..)?.find(&near).map(|at| line_start + at).or_else(|| sql.find(&near))?;
    Some(sql[..found].chars().count())
}

// ---------- Logging in ----------

fn xor(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter().zip(b).map(|(x, y)| x ^ y).collect()
}

/// mysql_native_password: SHA1(password) XOR SHA1(scramble + SHA1(SHA1(password))).
pub fn native_password(password: &str, scramble: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        return Vec::new();
    }
    let once = Sha1::digest(password.as_bytes());
    let twice = Sha1::digest(once);
    let mixed = Sha1::new().chain_update(scramble).chain_update(twice).finalize();
    xor(&once, &mixed)
}

/// caching_sha2_password's fast answer: SHA256(password) XOR SHA256(SHA256(SHA256(password)) + scramble).
pub fn caching_sha2_password(password: &str, scramble: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        return Vec::new();
    }
    let once = Sha256::digest(password.as_bytes());
    let twice = Sha256::digest(once);
    let mixed = Sha256::new().chain_update(twice).chain_update(scramble).finalize();
    xor(&once, &mixed)
}

/// The answer a login plugin wants, or why it can't be given.
fn plugin_answer(plugin: &str, password: &str, scramble: &[u8], tls: bool) -> Result<Vec<u8>, DbError> {
    // The scramble is 20 bytes; some servers send a zero after it.
    let scramble = &scramble[..scramble.len().min(20)];
    match plugin {
        "mysql_native_password" | "" => Ok(native_password(password, scramble)),
        "caching_sha2_password" => Ok(caching_sha2_password(password, scramble)),
        "mysql_clear_password" if tls => Ok([password.as_bytes(), &[0]].concat()),
        "mysql_clear_password" => Err(DbError::lost(crate::i18n::tr("The server wants the password sent as it is, which Limoniter only does over TLS. Set the connection to use TLS."))),
        other => Err(DbError::lost(crate::i18n::tf!("The server asks for a login Limoniter doesn't know: {other}."))),
    }
}

/// The packet that asks for TLS: the start of the login packet, with the SSL flag.
pub(super) fn ssl_request(capabilities: u32) -> Vec<u8> {
    let mut out = capabilities.to_le_bytes().to_vec();
    out.extend_from_slice(&MAX_PACKET.to_le_bytes());
    out.push(UTF8MB4);
    out.extend_from_slice(&[0; 23]);
    out
}

/// The login packet (HandshakeResponse41).
pub(super) fn login_packet(capabilities: u32, user: &str, answer: &[u8], database: &str, plugin: &str) -> Vec<u8> {
    let mut out = ssl_request(capabilities);
    out.extend_from_slice(user.as_bytes());
    out.push(0);
    out.extend_from_slice(&lenenc(answer.len() as u64));
    out.extend_from_slice(answer);
    if capabilities & CONNECT_WITH_DB != 0 {
        out.extend_from_slice(database.as_bytes());
        out.push(0);
    }
    out.extend_from_slice(plugin.as_bytes());
    out.push(0);
    out
}

impl MySql {
    fn read_packet(&mut self) -> Result<Vec<u8>, DbError> {
        let mut whole = Vec::new();
        loop {
            let mut head = [0u8; 4];
            self.stream.read_exact(&mut head).map_err(DbError::io)?;
            let length = usize::from(head[0]) | usize::from(head[1]) << 8 | usize::from(head[2]) << 16;
            self.sequence = head[3].wrapping_add(1);
            let start = whole.len();
            whole.resize(start + length, 0);
            self.stream.read_exact(&mut whole[start..]).map_err(DbError::io)?;
            // A packet of the most a packet can hold is followed by the rest of it.
            if length < 0xFF_FFFF {
                return Ok(whole);
            }
        }
    }

    fn write_packet(&mut self, payload: &[u8]) -> Result<(), DbError> {
        let mut out = Vec::with_capacity(payload.len() + 4);
        let mut chunks = payload.chunks(0xFF_FFFF).peekable();
        if payload.is_empty() {
            out.extend_from_slice(&[0, 0, 0, self.sequence]);
            self.sequence = self.sequence.wrapping_add(1);
        }
        while let Some(chunk) = chunks.next() {
            out.extend_from_slice(&(chunk.len() as u32).to_le_bytes()[..3]);
            out.push(self.sequence);
            self.sequence = self.sequence.wrapping_add(1);
            out.extend_from_slice(chunk);
            if chunks.peek().is_none() && chunk.len() == 0xFF_FFFF {
                out.extend_from_slice(&[0, 0, 0, self.sequence]);
                self.sequence = self.sequence.wrapping_add(1);
            }
        }
        self.stream.write_all(&out).and_then(|()| self.stream.flush()).map_err(DbError::io)
    }

    /// Sends a command, which starts a new exchange of packets.
    fn command(&mut self, command: u8, body: &[u8]) -> Result<(), DbError> {
        self.sequence = 0;
        self.write_packet(&[&[command][..], body].concat())
    }

    pub fn connect(connection: &Connection, password: &str) -> Result<MySql, DbError> {
        let stream = Stream::connect(&connection.host, connection.port).map_err(DbError::lost)?;
        let mut me = MySql { stream, sequence: 0, connection: connection.clone(), password: password.to_string(), thread: 0, version: String::new() };
        let greeting = read_greeting(&me.read_packet()?)?;
        me.thread = greeting.thread;
        me.version = greeting.version.clone();
        if greeting.capabilities & PROTOCOL_41 == 0 {
            return Err(DbError::lost(crate::i18n::tr("The server speaks a version of MySQL's protocol older than Limoniter knows.")));
        }
        let mut capabilities = LONG_PASSWORD | LONG_FLAG | PROTOCOL_41 | TRANSACTIONS | SECURE_CONNECTION | MULTI_STATEMENTS | MULTI_RESULTS | PLUGIN_AUTH | PLUGIN_AUTH_LENENC;
        if !connection.database.is_empty() {
            capabilities |= CONNECT_WITH_DB;
        }
        capabilities &= greeting.capabilities | CONNECT_WITH_DB;
        let offers_tls = greeting.capabilities & SSL != 0;
        if connection.tls == TlsMode::Require && !offers_tls {
            return Err(DbError::lost(crate::i18n::tf!("{} doesn't offer TLS, and this connection is set always to use it.", connection.host)));
        }
        if connection.tls != TlsMode::Disable && offers_tls {
            capabilities |= SSL;
            me.write_packet(&ssl_request(capabilities))?;
            let plain = std::mem::replace(&mut me.stream, Stream::Closed);
            me.stream = plain.start_tls(&connection.host, connection.trust_own_certificate).map_err(DbError::lost)?;
        }
        let tls = me.stream.is_tls();
        let plugin = if greeting.plugin.is_empty() { "mysql_native_password".to_string() } else { greeting.plugin.clone() };
        let answer = plugin_answer(&plugin, password, &greeting.scramble, tls)?;
        me.write_packet(&login_packet(capabilities, &connection.user, &answer, &connection.database, &plugin))?;
        me.finish_login(&plugin)?;
        Ok(me)
    }

    /// Reads the server's answers to the login until it's accepted or refused: switching plugin, and
    /// caching_sha2_password's fast or full login.
    fn finish_login(&mut self, plugin: &str) -> Result<(), DbError> {
        let mut plugin = plugin.to_string();
        loop {
            let packet = self.read_packet()?;
            match packet.first() {
                Some(0x00) => return Ok(()),
                Some(0xFF) => {
                    let mut problem = error_from(&packet);
                    problem.lost = true;
                    return Err(problem);
                }
                // An auth switch: another plugin, with a new scramble.
                Some(0xFE) => {
                    let mut reader = Reader::new(&packet[1..]);
                    plugin = String::from_utf8_lossy(reader.string()?).into_owned();
                    let scramble = reader.rest().to_vec();
                    let answer = plugin_answer(&plugin, &self.password, &scramble, self.stream.is_tls())?;
                    self.write_packet(&answer)?;
                }
                // More data from the plugin: caching_sha2_password's 3 (logged in from the cache, an OK follows)
                // or 4 (it wants the whole password).
                Some(0x01) if plugin == "caching_sha2_password" => match packet.get(1) {
                    Some(3) => {}
                    Some(4) if self.stream.is_tls() => {
                        let password = [self.password.as_bytes(), &[0]].concat();
                        self.write_packet(&password)?;
                    }
                    Some(4) => {
                        return Err(DbError::lost(crate::i18n::tr(
                            "The server wants the whole password (caching_sha2_password's full login), which needs TLS, and the connection isn't using it. Set the connection to use TLS, or log in once with the mysql program so the server keeps the password in its cache.",
                        )));
                    }
                    _ => return Err(unreadable()),
                },
                _ => return Err(unreadable()),
            }
        }
    }

    /// A result set's column definitions, then its rows, after the column count.
    fn result_set(&mut self, count: usize, most: usize) -> Result<(Outcome, u16), DbError> {
        let mut columns = Vec::with_capacity(count);
        for _ in 0..count {
            let packet = self.read_packet()?;
            columns.push(column_definition(&packet)?);
        }
        // The column definitions end with an EOF packet.
        let packet = self.read_packet()?;
        if !is_eof(&packet) {
            return Err(unreadable());
        }
        let mut rows = Gathering::new(columns.iter().map(|(column, _)| column.clone()).collect(), most);
        loop {
            let packet = self.read_packet()?;
            if is_eof(&packet) {
                let status = u16::from_le_bytes([packet.get(3).copied().unwrap_or(0), packet.get(4).copied().unwrap_or(0)]);
                return Ok((rows.finish(), status));
            }
            if packet.first() == Some(&0xFF) {
                return Err(error_from(&packet));
            }
            if rows.wants_more() {
                rows.push(text_row(&packet, &columns)?);
            } else {
                rows.more += 1;
            }
        }
    }
}

/// An EOF packet: 0xFE, and short, which a row starting with a long length never is.
fn is_eof(packet: &[u8]) -> bool {
    packet.first() == Some(&0xFE) && packet.len() < 9
}

/// An OK packet's rows changed and status.
fn ok_packet(packet: &[u8]) -> Result<(u64, u16, String), DbError> {
    let mut reader = Reader::new(&packet[1..]);
    let rows = reader.lenenc()?.unwrap_or(0);
    let _insert_id = reader.lenenc()?;
    let status = reader.u16()?;
    let _warnings = reader.u16()?;
    let info = String::from_utf8_lossy(reader.rest()).into_owned();
    Ok((rows, status, info))
}

/// A column definition (protocol 4.1): its name, kind and whether its bytes are binary.
pub(super) fn column_definition(packet: &[u8]) -> Result<(DataColumn, bool), DbError> {
    let mut reader = Reader::new(packet);
    for _ in 0..4 {
        reader.lenenc_bytes()?; // catalog, schema, table, original table
    }
    let name = reader.lenenc_string()?;
    reader.lenenc_bytes()?; // original name
    reader.lenenc()?; // the fixed fields' length, 12
    let charset = reader.u16()?;
    let _length = reader.u32()?;
    let kind = reader.u8()?;
    Ok((DataColumn { name, kind: kind_of(kind, charset == BINARY) }, charset == BINARY))
}

/// What a column holds, from its type number, and whether its character set is binary.
pub fn kind_of(kind: u8, binary: bool) -> ColumnKind {
    match kind {
        1 | 2 | 3 | 8 | 9 | 13 => ColumnKind::Integer,
        0 | 4 | 5 | 246 => ColumnKind::Real,
        10 | 14 => ColumnKind::Date,
        7 | 12 => ColumnKind::DateTime,
        245 => ColumnKind::Json,
        16 | 255 => ColumnKind::Binary,
        15 | 249..=254 if binary => ColumnKind::Binary,
        11 | 15 | 247 | 248 | 249..=254 => ColumnKind::Text,
        _ => ColumnKind::Unknown,
    }
}

fn text_row(packet: &[u8], columns: &[(DataColumn, bool)]) -> Result<Vec<Cell>, DbError> {
    let mut reader = Reader::new(packet);
    let mut row = Vec::with_capacity(columns.len());
    for (column, _) in columns {
        row.push(match reader.lenenc_bytes()? {
            None => Cell::Null,
            // Numbers and dates carry the binary character set too, so it's the kind that says.
            Some(bytes) if column.kind == ColumnKind::Binary => Cell::Bytes(bytes.to_vec()),
            Some(bytes) => super::cell_from_text(column.kind, &String::from_utf8_lossy(bytes)),
        });
    }
    Ok(row)
}

impl Client for MySql {
    fn run(&mut self, sql: &str, most: usize) -> Result<Reply, DbError> {
        self.command(COM_QUERY, sql.as_bytes())?;
        let mut reply = Reply::default();
        loop {
            let packet = self.read_packet()?;
            let status = match packet.first() {
                Some(0x00) => {
                    let (rows, status, info) = ok_packet(&packet)?;
                    reply.outcomes.push(Outcome::Done { command: String::new(), rows: Some(rows) });
                    if !info.is_empty() {
                        reply.notices.push(info);
                    }
                    status
                }
                Some(0xFF) => {
                    let mut problem = error_from(&packet);
                    problem.position = error_position(&problem.message, sql);
                    problem.before = reply;
                    return Err(problem);
                }
                Some(0xFB) => {
                    let _ = self.write_packet(&[]);
                    return Err(DbError::new(crate::i18n::tr("LOAD DATA LOCAL INFILE isn't something Limoniter sends files for.")));
                }
                Some(_) => {
                    let count = Reader::new(&packet).lenenc()?.unwrap_or(0) as usize;
                    let (rows, status) = self.result_set(count, most).map_err(|mut problem| {
                        problem.position = problem.position.or_else(|| error_position(&problem.message, sql));
                        problem
                    })?;
                    reply.outcomes.push(rows);
                    status
                }
                None => return Err(unreadable()),
            };
            if status & MORE_RESULTS == 0 {
                return Ok(reply);
            }
        }
    }

    fn catalogue(&mut self) -> Result<Vec<Schema>, DbError> {
        const TABLES: &str = "SELECT t.TABLE_SCHEMA, t.TABLE_NAME, t.TABLE_TYPE = 'VIEW', coalesce(c.COLUMN_NAME, ''), coalesce(c.COLUMN_TYPE, ''), coalesce(c.COLUMN_KEY, '') \
            FROM information_schema.TABLES t LEFT JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME \
            WHERE t.TABLE_SCHEMA NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys') \
            ORDER BY t.TABLE_SCHEMA = DATABASE() DESC, t.TABLE_SCHEMA, t.TABLE_NAME, c.ORDINAL_POSITION";
        const FOREIGN: &str = "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, concat(REFERENCED_TABLE_SCHEMA, '.', REFERENCED_TABLE_NAME, '(', REFERENCED_COLUMN_NAME, ')') \
            FROM information_schema.KEY_COLUMN_USAGE WHERE REFERENCED_TABLE_NAME IS NOT NULL";
        let rows = super::rows_of(self.run(TABLES, usize::MAX)?);
        // TABLE_TYPE = 'VIEW' comes back as a number.
        let is_view = |cell: Option<&Cell>| matches!(cell, Some(Cell::Int(1))) || super::text_of(cell) == "1";
        let mut schemas = super::catalogue_from_rows(rows.iter().map(|r| (super::text_of(r.first()), super::text_of(r.get(1)), is_view(r.get(2)), super::text_of(r.get(3)), super::text_of(r.get(4)))));
        let primary: Vec<_> = rows.iter().filter(|r| super::text_of(r.get(5)) == "PRI").map(|r| (super::text_of(r.first()), super::text_of(r.get(1)), super::text_of(r.get(3)))).collect();
        let foreign: Vec<_> = super::rows_of(self.run(FOREIGN, usize::MAX)?).iter().map(|r| (super::text_of(r.first()), super::text_of(r.get(1)), super::text_of(r.get(2)), super::text_of(r.get(3)))).collect();
        super::mark_keys(&mut schemas, &primary, &foreign);
        Ok(schemas)
    }

    fn stopper(&self) -> Stopper {
        Stopper::MySql { connection: self.connection.clone(), password: self.password.clone(), thread: self.thread }
    }

    /// Changes the database the connection is using, as USE does.
    fn use_database(&mut self, database: &str) -> Result<(), DbError> {
        self.command(COM_INIT_DB, database.as_bytes())?;
        let packet = self.read_packet()?;
        if packet.first() == Some(&0xFF) { Err(error_from(&packet)) } else { Ok(()) }
    }

    fn server(&self) -> String {
        if self.version.to_ascii_lowercase().contains("mariadb") { format!("MariaDB {}", self.version.split('-').next().unwrap_or_default()) } else { format!("MySQL {}", self.version) }
    }
}

impl Drop for MySql {
    fn drop(&mut self) {
        let _ = self.command(COM_QUIT, &[]);
    }
}

/// Stops the query running on another connection: KILL QUERY, sent from a connection of its own.
pub fn kill_query(connection: &Connection, password: &str, thread: u32) -> Result<(), String> {
    let mut other = MySql::connect(connection, password).map_err(|e| e.message)?;
    other.run(&format!("KILL QUERY {thread}"), 0).map(|_| ()).map_err(|e| e.message)
}

#[cfg(test)]
#[path = "mysql_tests.rs"]
mod tests;
