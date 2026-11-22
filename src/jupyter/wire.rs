//! Jupyter's messaging protocol (version 5.3) on top of ZeroMQ messages: the identities, the `<IDS|MSG>`
//! delimiter, an HMAC-SHA256 signature made with the connection file's key, and then four JSON parts (header,
//! parent header, metadata and content) and any binary buffers.

use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

/// The frame that separates the routing identities from the message itself.
pub const DELIMITER: &[u8] = b"<IDS|MSG>";

/// The protocol version spoken, as headers give it.
pub const PROTOCOL_VERSION: &str = "5.3";

/// One Jupyter message.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// What came before the delimiter (a ROUTER's routing identities, or a PUB's topic).
    pub ids: Vec<Vec<u8>>,
    pub header: Value,
    pub parent_header: Value,
    pub metadata: Value,
    pub content: Value,
    pub buffers: Vec<Vec<u8>>,
}

impl Message {
    pub fn msg_type(&self) -> &str {
        self.header["msg_type"].as_str().unwrap_or_default()
    }

    pub fn msg_id(&self) -> &str {
        self.header["msg_id"].as_str().unwrap_or_default()
    }

    /// The msg_id of the request this answers, or "" when it answers none.
    pub fn parent_id(&self) -> &str {
        self.parent_header["msg_id"].as_str().unwrap_or_default()
    }
}

/// HMAC with SHA-256 (RFC 2104) over the parts one after another, as Python's `hmac` module makes it.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5C));
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 128 bits nobody outside can guess. The standard library's hasher keys come from the system's random source,
/// and each `RandomState` has fresh ones, so hashing a counter and the time with two of them gives what a
/// version 4 UUID needs without another crate.
pub fn random_bytes() -> [u8; 16] {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let time = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut bytes = [0u8; 16];
    for half in bytes.chunks_mut(8) {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(count);
        hasher.write_u128(time);
        hasher.write_u32(std::process::id());
        half.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes
}

/// A new version 4 UUID, written as Jupyter writes its message and session ids.
pub fn new_id() -> String {
    let mut b = random_bytes();
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let h = hex(&b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// A key for signing messages, as jupyter_client makes one: a UUID's 32 hex digits.
pub fn new_key() -> String {
    hex(&random_bytes())
}

/// A time in ISO 8601, in UTC to the microsecond, as Jupyter dates its headers ("2026-11-22T09:30:00.123456Z").
pub fn iso_date(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = since.as_secs();
    let (days, rest) = ((seconds / 86_400) as i64, seconds % 86_400);
    // Howard Hinnant's days-to-civil, counting in eras of 400 years from 1 March 0000.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z", rest / 3600, rest / 60 % 60, rest % 60, since.subsec_micros())
}

/// Who is talking to the kernel: a session id, the signing key and the user's name.
pub struct Session {
    pub id: String,
    key: Vec<u8>,
    username: String,
}

impl Session {
    pub fn new(key: &str) -> Session {
        let username = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "limoniter".into());
        Session { id: new_id(), key: key.as_bytes().to_vec(), username }
    }

    pub fn key(&self) -> &str {
        std::str::from_utf8(&self.key).unwrap_or_default()
    }

    /// A header for a new message of this type.
    pub fn header(&self, msg_type: &str) -> Value {
        json!({
            "msg_id": new_id(),
            "session": self.id,
            "username": self.username,
            "date": iso_date(SystemTime::now()),
            "msg_type": msg_type,
            "version": PROTOCOL_VERSION,
        })
    }

    /// A new message, answering `parent` (a header, or `{}` for none).
    pub fn make_msg(&self, msg_type: &str, content: Value, parent: Value) -> Message {
        Message { ids: Vec::new(), header: self.header(msg_type), parent_header: parent, metadata: json!({}), content, buffers: Vec::new() }
    }

    /// The signature of the four JSON parts, in lower-case hex; empty when there's no key, as Jupyter allows.
    pub fn sign(&self, parts: &[&[u8]]) -> String {
        if self.key.is_empty() { String::new() } else { hex(&hmac_sha256(&self.key, parts)) }
    }

    /// The message as ZeroMQ frames.
    pub fn encode(&self, message: &Message) -> Vec<Vec<u8>> {
        let json = |value: &Value| serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
        let parts = [json(&message.header), json(&message.parent_header), json(&message.metadata), json(&message.content)];
        let signature = self.sign(&[&parts[0], &parts[1], &parts[2], &parts[3]]);
        let mut frames = message.ids.clone();
        frames.push(DELIMITER.to_vec());
        frames.push(signature.into_bytes());
        frames.extend(parts);
        frames.extend(message.buffers.iter().cloned());
        frames
    }

    /// A message read from ZeroMQ frames, refused if its signature isn't this key's.
    pub fn decode(&self, mut frames: Vec<Vec<u8>>) -> Result<Message, String> {
        let at = frames.iter().position(|f| f == DELIMITER).ok_or("no <IDS|MSG> delimiter")?;
        if frames.len() < at + 6 {
            return Err("too few parts".into());
        }
        let buffers = frames.split_off(at + 6);
        let content = frames.pop().unwrap_or_default();
        let metadata = frames.pop().unwrap_or_default();
        let parent = frames.pop().unwrap_or_default();
        let header = frames.pop().unwrap_or_default();
        let signature = frames.pop().unwrap_or_default();
        frames.pop();
        if !self.key.is_empty() {
            let expected = self.sign(&[&header, &parent, &metadata, &content]).into_bytes();
            let given = signature.to_ascii_lowercase();
            // Compared in full every time, so the time taken says nothing of how much matched.
            let same = expected.len() == given.len() && expected.iter().zip(&given).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0;
            if !same {
                return Err("the signature is wrong".into());
            }
        }
        let parse = |bytes: &[u8]| serde_json::from_slice::<Value>(bytes).map_err(|e| e.to_string());
        Ok(Message { ids: frames, header: parse(&header)?, parent_header: parse(&parent)?, metadata: parse(&metadata)?, content: parse(&content)?, buffers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn hmac_matches_rfc_4231_and_python() {
        assert_eq!(hex(&hmac_sha256(b"Jefe", &[b"what do ya want for nothing?"])), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // Split across parts it's the same, as Python's hmac.update makes it.
        assert_eq!(hex(&hmac_sha256(b"Jefe", &[b"what do ya ", b"want for nothing?"])), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // A key longer than the block is hashed first (worked out with Python's hmac).
        assert_eq!(hex(&hmac_sha256(&[b'k'; 100], &[b"abc"])), "b58b2b694fdba0dd76da3ebe99174f728d327560f36ece224e90867972479922");
    }

    #[test]
    fn a_message_is_signed_as_jupyter_signs_it() {
        let session = Session::new("secret");
        // hmac.new(b"secret", header + parent + metadata + content, sha256), worked out with Python.
        let signature = session.sign(&[br#"{"msg_id":"a"}"#, b"{}", b"{}", br#"{"code":"1+1"}"#]);
        assert_eq!(signature, "2b4fcfd5e346a79b74b969e4967bc74f062ac8e372d93dea5612ac8efc684746");
        assert_eq!(Session::new("").sign(&[b"x"]), "");
    }

    #[test]
    fn messages_go_through_frames_and_back() {
        let session = Session::new("secret");
        let mut message = session.make_msg("execute_request", json!({"code": "1+1"}), json!({}));
        message.ids = vec![b"someone".to_vec()];
        message.buffers = vec![vec![1, 2, 3]];
        let frames = session.encode(&message);
        assert_eq!(frames[0], b"someone");
        assert_eq!(frames[1], DELIMITER);
        assert_eq!(frames[2].len(), 64);
        assert_eq!(frames[3], serde_json::to_vec(&message.header).unwrap());
        assert_eq!(frames[4], b"{}");
        let back = session.decode(frames.clone()).unwrap();
        assert_eq!(back, message);
        assert_eq!(back.msg_type(), "execute_request");
        assert_eq!(back.header["version"], "5.3");
        assert_eq!(back.header["session"], session.id.as_str());

        // A changed content, or another key, is refused.
        let mut tampered = frames.clone();
        tampered[6] = br#"{"code":"import os"}"#.to_vec();
        assert!(session.decode(tampered).is_err());
        assert!(Session::new("other").decode(frames.clone()).is_err());
        // Without a key nothing is checked.
        assert!(Session::new("").decode(frames).is_ok());
        assert!(session.decode(vec![b"x".to_vec()]).is_err());
    }

    #[test]
    fn dates_and_ids_are_as_jupyter_writes_them() {
        assert_eq!(iso_date(UNIX_EPOCH), "1970-01-01T00:00:00.000000Z");
        // Worked out with Python's datetime; leap days and microseconds come through.
        assert_eq!(iso_date(UNIX_EPOCH + Duration::from_micros(1_795_338_645_123_456)), "2026-11-22T09:10:45.123456Z");
        assert_eq!(iso_date(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00.000000Z");
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.as_bytes()[14], b'4');
        assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        assert_ne!(new_id(), id);
        assert_eq!(new_key().len(), 32);
    }
}
