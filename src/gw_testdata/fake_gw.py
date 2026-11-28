# A stand-in for Greaseweazle's `gw` program, for the tests of Limoniter's Greaseweazle windows: no
# Greaseweazle is plugged in here. It prints what gw 1.23 prints (the words and shapes are from its source: tools/info.py,
# read.py, write.py, rpm.py, clean.py, erase.py, update.py and cli.py), all on stderr as gw does, and for a read
# writes the image given by --image to the file asked for.
#
# Options before gw's own: --log FILE (every argument it was given), --scenario NAME, --image FILE.
# Scenarios: good, badsector (a track with a missing sector, retried), nodevice, denied, nodisk, protected.
import sys

ARGS = sys.argv[1:]


def take(name):
    if name in ARGS:
        at = ARGS.index(name)
        value = ARGS[at + 1]
        del ARGS[at:at + 2]
        return value
    return None


LOG = take("--log")
SCENARIO = take("--scenario") or "good"
IMAGE = take("--image")
if LOG:
    with open(LOG, "a") as f:
        f.write(" ".join(ARGS) + "\n")


def say(text="", end="\n"):
    sys.stderr.write(text + end)
    sys.stderr.flush()


def fatal(text):
    say("** FATAL ERROR:")
    say(text)
    sys.exit(1)


def option(name, default=None):
    if name in ARGS:
        return ARGS[ARGS.index(name) + 1]
    return default


def check_device():
    if SCENARIO == "nodevice":
        fatal("Cannot find the Greaseweazle device")
    if SCENARIO == "denied":
        fatal("[Errno 13] could not open port /dev/ttyACM0: [Errno 13] Permission denied: '/dev/ttyACM0'")


if not ARGS:
    sys.exit(1)
action = ARGS[0]

if action == "info":
    say("Host Tools: 1.23")
    say("Device:")
    if SCENARIO in ("nodevice", "denied"):
        if SCENARIO == "denied":
            check_device()
        say("  Not found")
        sys.exit(0)
    say("  Port:     /dev/ttyACM0")
    say("  Model:    Greaseweazle F7 Lightning")
    say("  MCU:      AT32F403A, 240MHz, 96kB SRAM")
    say("  Firmware: 1.6")
    say("  Serial:   GW0123456789")
    say("  USB:      Full Speed (12 Mbit/s), 64kB Buffer")
    say("")
    say("*** New firmware version 1.7 is available")
    say("To perform an Update:")
    say(" - Disconnect from USB")
    sys.exit(0)

check_device()
if SCENARIO == "nodisk" and action in ("read", "write", "rpm", "erase"):
    say("Reading c=0-79:h=0-1 revs=3" if action == "read" else "Writing c=0-79:h=0-1")
    say("Command Failed: %s: No Index" % action.capitalize())
    sys.exit(0)

if action == "rpm":
    for n in range(3):
        say("Rate: 299.8%d rpm ; Period: 200.1%d ms" % (n, n))
    say("***")
    say("FASTEST:  Rate: 300.010 rpm ; Period: 199.993 ms")
    say("Ar.Mean:  Rate: 299.900 rpm ; Period: 200.067 ms")
    say("Median:   Rate: 299.862 rpm ; Period: 200.091 ms")
    say("SLOWEST:  Rate: 299.810 rpm ; Period: 200.126 ms")
elif action == "clean":
    for p in range(3):
        say("Pass %d: " % p, end="")
        say("9 0 19 10 29 20 39 30 49 40 59 50 69 60 79 70")
elif action == "erase":
    say("Erasing c=0-1:h=0-1, revs=1")
    for c in range(2):
        for h in range(2):
            say("T%d.%d: Erasing Track" % (c, h))
elif action == "update":
    say("Updating Main Firmware to version 1.7...")
    say("Downloading latest firmware: Greaseweazle-F7-v1.7.upd")
    say("Done.")
elif action == "write":
    if SCENARIO == "protected":
        say("Writing c=0-79:h=0-1")
        say("Command Failed: Write: Disk is Write Protected")
        sys.exit(0)
    fmt = option("--format")
    say("Format %s" % fmt if fmt else "")
    say("Writing c=0-1:h=0-1")
    for c in range(2):
        for h in range(2):
            say("T%d.%d: Writing Track (Amiga (11/11 sectors))" % (c, h))
    if "--no-verify" in ARGS:
        say("No tracks verified (Reason: Verify disabled)")
    else:
        say("All tracks verified")
elif action == "read":
    fmt = option("--format")
    path = ARGS[-1]
    say("Reading c=0-1:h=0-1 revs=%s" % option("--revs", "2"))
    if fmt:
        say("Format " + fmt)
    sectors = 11
    for c in range(2):
        for h in range(2):
            if SCENARIO == "badsector" and (c, h) == (1, 0):
                say("T1.0: Amiga (10/11 sectors) from Raw Flux (101240 flux samples)")
                say("T1.0: Amiga (10/11 sectors) from Raw Flux (101302 flux samples) (Retry #0.1)")
                say("T1.0: Amiga (10/11 sectors) from Raw Flux (101277 flux samples) (Retry #0.2)")
                say("T1.0: Amiga (10/11 sectors) from Raw Flux (101291 flux samples) (Retry #0.3)")
                say("T1.0: Giving up: 1 sectors missing")
            else:
                say("T%d.%d: Amiga (%d/%d sectors) from Raw Flux (101302 flux samples)" % (c, h, sectors, sectors))
    if fmt:
        bad = 1 if SCENARIO == "badsector" else 0
        say("Cyl-> 0 ")
        say("H. S: 01")
        total = 4 * sectors
        say("Found %d sectors of %d (%d%%)" % (total - bad, total, (total - bad) * 100 // total))
    data = open(IMAGE, "rb").read() if IMAGE else bytes(901120)
    with open(path, "wb") as f:
        f.write(data)
sys.exit(0)
