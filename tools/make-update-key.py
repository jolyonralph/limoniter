#!/usr/bin/env python3
"""Makes the key that Limoniter's updates are signed with, and signs a release's files with it.

Limoniter only installs an update whose list of SHA-256 fingerprints (SHA256SUMS.txt) is signed by this key, so a
download that was changed on its way, or a release made by someone who got into the GitHub account but hasn't got
the key, is refused. The signatures are minisign's (https://jedisct1.github.io/minisign/), in its Ed25519 form, so
`minisign -V` can check them too.

Once, on your own computer:

    python3 tools/make-update-key.py new limoniter-update.key

It writes the secret key to that file (keep it safe and offline: anyone with it can sign updates that every
Limoniter will install) and prints the public key, which goes in src/self_update.rs as PUBLIC_KEY. Then give the
secret key to GitHub, as the repository secret LIMONITER_UPDATE_KEY:

    gh secret set LIMONITER_UPDATE_KEY < limoniter-update.key

The Release workflow then signs each release:

    python3 tools/make-update-key.py sign SHA256SUMS.txt      (the key from $LIMONITER_UPDATE_KEY)
    python3 tools/make-update-key.py sign --key limoniter-update.key SHA256SUMS.txt

and checks first that the key built into Limoniter is this one's other half (`check src/self_update.rs`). Anyone
can check a signature against the public key:

    python3 tools/make-update-key.py verify "RWQ...public key..." SHA256SUMS.txt

It needs nothing but Python 3.6 or later: Ed25519 is worked out here, as RFC 8032 sets it out, so nothing has to be
installed. That way is slow (a fraction of a second a signature), which doesn't matter for a few small files.
"""

import base64
import hashlib
import os
import re
import sys
import time

# Ed25519, from RFC 8032 section 6 (its reference code, made a little plainer).
P = 2 ** 255 - 19
L = 2 ** 252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
I = pow(2, (P - 1) // 4, P)


def _add(a, b):
    """Adds two points, in extended coordinates."""
    A = (a[1] - a[0]) * (b[1] - b[0]) % P
    B = (a[1] + a[0]) * (b[1] + b[0]) % P
    C = 2 * a[3] * b[3] * D % P
    Dd = 2 * a[2] * b[2] % P
    E, F, G, H = B - A, Dd - C, Dd + C, B + A
    return (E * F % P, G * H % P, F * G % P, E * H % P)


def _times(s, point):
    result = (0, 1, 1, 0)
    while s > 0:
        if s & 1:
            result = _add(result, point)
        point = _add(point, point)
        s >>= 1
    return result


def _equal(a, b):
    return (a[0] * b[2] - b[0] * a[2]) % P == 0 and (a[1] * b[2] - b[1] * a[2]) % P == 0


def _recover_x(y, sign):
    if y >= P:
        return None
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P)
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P != 0:
        x = x * I % P
    if (x * x - x2) % P != 0:
        return None
    if (x & 1) != sign:
        x = P - x
    return x


GY = 4 * pow(5, P - 2, P) % P
GX = _recover_x(GY, 0)
G = (GX, GY, 1, GX * GY % P)


def _compress(point):
    zinv = pow(point[2], P - 2, P)
    x, y = point[0] * zinv % P, point[1] * zinv % P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _decompress(data):
    if len(data) != 32:
        return None
    y = int.from_bytes(data, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign)
    if x is None:
        return None
    return (x, y, 1, x * y % P)


def _sha512_int(data):
    return int.from_bytes(hashlib.sha512(data).digest(), "little")


def _expand(seed):
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def public_of(seed):
    a, _ = _expand(seed)
    return _compress(_times(a, G))


def sign_bytes(seed, message):
    a, prefix = _expand(seed)
    public = _compress(_times(a, G))
    r = _sha512_int(prefix + message) % L
    big_r = _compress(_times(r, G))
    h = _sha512_int(big_r + public + message) % L
    s = (r + h * a) % L
    return big_r + int.to_bytes(s, 32, "little")


def verify_bytes(public, message, signature):
    if len(public) != 32 or len(signature) != 64:
        return False
    a = _decompress(public)
    big_r = _decompress(signature[:32])
    if a is None or big_r is None:
        return False
    s = int.from_bytes(signature[32:], "little")
    if s >= L:
        return False
    h = _sha512_int(signature[:32] + public + message) % L
    return _equal(_times(s, G), _add(big_r, _times(h, a)))


# minisign's files. The algorithm is "Ed": the file itself is signed (minisign's older form, which every minisign
# still checks), rather than its BLAKE2b hash, so Limoniter needs nothing beyond Ed25519 to check it.
ALGORITHM = b"Ed"
SECRET_HEADER = "untrusted comment: Limoniter's update signing key. Keep it secret: anyone who has it can sign updates."


def _b64(data):
    return base64.b64encode(data).decode("ascii")


def _last_line(text):
    lines = [line.strip() for line in text.splitlines() if line.strip() and not line.startswith("untrusted comment:")]
    if not lines:
        raise SystemExit("There's no key in that.")
    return lines[-1]


def read_secret(text):
    """The key id and seed from a secret key file's text."""
    data = base64.b64decode(_last_line(text))
    if len(data) != 2 + 8 + 32 or data[:2] != ALGORITHM:
        raise SystemExit("That isn't a key made by make-update-key.py.")
    return data[2:10], data[10:]


def public_key_text(key_id, public):
    return _b64(ALGORITHM + key_id + public)


def read_public(text):
    data = base64.b64decode(_last_line(text))
    if len(data) != 2 + 8 + 32 or data[:2] != ALGORITHM:
        raise SystemExit("That isn't a minisign Ed25519 public key.")
    return data[2:10], data[10:]


def signature_text(key_id, seed, message, file_name, timestamp=None):
    """A minisign signature file for `message`: the signature, and its trusted comment signed with it."""
    signature = sign_bytes(seed, message)
    timestamp = int(time.time()) if timestamp is None else timestamp
    trusted = "timestamp:%d\tfile:%s" % (timestamp, file_name)
    global_signature = sign_bytes(seed, signature + trusted.encode("utf-8"))
    return "untrusted comment: signature from Limoniter's update key\n%s\ntrusted comment: %s\n%s\n" % (
        _b64(ALGORITHM + key_id + signature), trusted, _b64(global_signature))


def check_signature(public_text, message, signature_file_text):
    key_id, public = read_public(public_text)
    lines = signature_file_text.splitlines()
    if len(lines) < 4 or not lines[2].startswith("trusted comment: "):
        return "The signature file isn't minisign's."
    data = base64.b64decode(lines[1])
    if len(data) != 74 or data[:2] != ALGORITHM:
        return "The signature isn't an Ed25519 one this script makes."
    if data[2:10] != key_id:
        return "The signature was made with a different key."
    if not verify_bytes(public, message, data[10:]):
        return "The signature doesn't match the file."
    trusted = lines[2][len("trusted comment: "):].encode("utf-8")
    if not verify_bytes(public, data[10:] + trusted, base64.b64decode(lines[3])):
        return "The signature's trusted comment doesn't match it."
    return None


def main(argv):
    if len(argv) >= 2 and argv[0] == "new":
        path = argv[1]
        if os.path.exists(path):
            raise SystemExit("%s is there already; a new key would replace it, so choose another name." % path)
        seed, key_id = os.urandom(32), os.urandom(8)
        with open(path, "x", encoding="ascii") as out:
            out.write("%s\n%s\n" % (SECRET_HEADER, _b64(ALGORITHM + key_id + seed)))
        try:
            os.chmod(path, 0o600)
        except OSError:
            pass
        public = public_key_text(key_id, public_of(seed))
        print("The secret key is in %s. Keep it somewhere safe and offline, and never commit it." % path)
        print()
        print("The public key, for PUBLIC_KEY in src/self_update.rs:")
        print()
        print("    " + public)
        print()
        print("Give the secret key to GitHub as the repository secret LIMONITER_UPDATE_KEY:")
        print()
        print("    gh secret set LIMONITER_UPDATE_KEY < %s" % path)
        return 0
    if len(argv) >= 2 and argv[0] == "sign":
        files = argv[1:]
        if files[0] == "--key":
            if len(files) < 3:
                raise SystemExit("sign --key KEYFILE FILE...")
            with open(files[1], encoding="ascii") as kept:
                secret = kept.read()
            files = files[2:]
        else:
            secret = os.environ.get("LIMONITER_UPDATE_KEY", "")
            if not secret.strip():
                raise SystemExit("There's no key: give one with --key, or in LIMONITER_UPDATE_KEY.")
        key_id, seed = read_secret(secret)
        for path in files:
            with open(path, "rb") as source:
                message = source.read()
            with open(path + ".minisig", "w", encoding="utf-8", newline="\n") as out:
                out.write(signature_text(key_id, seed, message, os.path.basename(path)))
            print("Signed %s" % path)
        return 0
    if len(argv) == 3 and argv[0] == "verify":
        with open(argv[2], "rb") as source:
            message = source.read()
        with open(argv[2] + ".minisig", encoding="utf-8") as signature:
            problem = check_signature(argv[1], message, signature.read())
        print(problem or "The signature is good.")
        return 1 if problem else 0
    if len(argv) == 2 and argv[0] == "check":
        # That the key built into Limoniter is the secret key's other half, so what's signed will be installed.
        secret = os.environ.get("LIMONITER_UPDATE_KEY", "")
        if not secret.strip():
            raise SystemExit("There's no key in LIMONITER_UPDATE_KEY to check against.")
        key_id, seed = read_secret(secret)
        public = public_key_text(key_id, public_of(seed))
        with open(argv[1], encoding="utf-8") as source:
            built_in = re.search(r'pub const PUBLIC_KEY: &str = "([^"]*)";', source.read())
        if not built_in:
            raise SystemExit("%s has no PUBLIC_KEY." % argv[1])
        if built_in.group(1) != public:
            raise SystemExit("The key built into Limoniter (PUBLIC_KEY in %s) isn't the one LIMONITER_UPDATE_KEY signs "
                             "with, so nobody could install this release as an update. Put this there:\n\n    %s" % (argv[1], public))
        print("PUBLIC_KEY in %s is the signing key's." % argv[1])
        return 0
    if len(argv) == 2 and argv[0] == "public":
        with open(argv[1], encoding="ascii") as kept:
            key_id, seed = read_secret(kept.read())
        print(public_key_text(key_id, public_of(seed)))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
