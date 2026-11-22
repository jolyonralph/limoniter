#!/bin/sh
# A stand-in for `docker`, for Limoniter's tests: it prints what Docker Compose v2 prints, from the files beside it.
# Run as `sh docker.sh compose ...`; it keeps to POSIX sh so BSD and GNU systems both run it.
here=$(dirname "$0")
[ "$1" = "compose" ] || { echo "unknown command: $1" >&2; exit 1; }
shift
case "$1" in
    version) echo "Docker Compose version v2.29.1" ;;
    config) cat "$here/config.json" ;;
    ps)
        if [ -f "$here/ps-broken" ]; then echo "Cannot connect to the Docker daemon" >&2; exit 1; fi
        cat "$here/ps.ndjson" ;;
    logs) echo "web-1  | listening on 80"; echo "web-1  | GET / 200" ;;
    *) echo "ran $*" ;;
esac
