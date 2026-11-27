#!/bin/sh
# The limoniter command for this WSL distribution, put here by Limoniter (Settings: Limoniter command in WSL).
# It opens files in Limoniter on Windows. Taking it away is safe: delete this file.
# exe: __COMMENT__
exe_windows='__EXE__'

say() { printf '%s\n' "$*" >&2; }

# One file argument as Windows writes it, keeping a :LINE or :LINE:COLUMN on the end. Sets $converted_path and
# $converted_tail (the tail is what a window can be told separately).
convert() {
    converted_path=$1
    converted_tail=
    case $converted_path in
        *://*) return ;;
    esac
    if [ ! -e "$converted_path" ]; then
        count=0
        while [ "$count" -lt 2 ]; do
            last=${converted_path##*:}
            case $last in '' | *[!0-9]*) break ;; esac
            rest=${converted_path%:*}
            { [ -n "$rest" ] && [ "$rest" != "$converted_path" ]; } || break
            converted_path=$rest
            converted_tail=":$last$converted_tail"
            count=$((count + 1))
        done
    fi
    case $converted_path in -*) converted_path=./$converted_path ;; esac
    windows=$(wslpath -aw "$converted_path" 2>/dev/null) && converted_path=$windows
}

# Whether WSL will run Windows programs from here.
interop() {
    case ${LIMONITER_WSL_INTEROP:-} in
        yes) return 0 ;;
        no) return 1 ;;
    esac
    for entry in /proc/sys/fs/binfmt_misc/WSLInterop /proc/sys/fs/binfmt_misc/WSLInterop-late; do
        if [ -r "$entry" ] && grep -q '^enabled' "$entry" 2>/dev/null; then
            return 0
        fi
    done
    return 1
}

wait=
new_window=
line=
text_only=
options_done=
body=
n=$#
while [ "$n" -gt 0 ]; do
    arg=$1
    shift
    n=$((n - 1))
    if [ -z "$options_done" ]; then
        case $arg in
            --)
                options_done=1
                set -- "$@" "$arg"
                continue
                ;;
            --wait) wait=1 ;;
            --new-window) new_window=1 ;;
            --help | -h | --version | -V) text_only=1 ;;
            --line)
                set -- "$@" "$arg"
                if [ "$n" -gt 0 ]; then
                    line=$1
                    set -- "$@" "$1"
                    shift
                    n=$((n - 1))
                fi
                continue
                ;;
        esac
        case $arg in
            -*)
                set -- "$@" "$arg"
                continue
                ;;
        esac
    fi
    convert "$arg"
    set -- "$@" "$converted_path$converted_tail"
    # The same, as a window is asked for it over the network.
    if [ -n "$converted_tail" ]; then
        numbers=${converted_tail#:}
        body="$body@line ${numbers%%:*}
"
        case $numbers in *:*) body="$body@column ${numbers#*:}
" ;; esac
    elif [ -n "$line" ]; then
        body="$body@line $line
"
    fi
    body="$body$converted_path
"
done

# Run the Windows program, which hands the files to the window that's open (and with --wait, keeps going until
# they're closed).
if interop; then
    exe=$(wslpath -u "$exe_windows" 2>/dev/null) || exe=
    if [ -z "$exe" ] || [ ! -x "$exe" ]; then
        say "Limoniter isn't where this command expects it: $exe_windows"
        say "Start Limoniter from Windows once, and it will put this command right."
        exit 1
    fi
    "$exe" "$@"
    exit $?
fi

# Without that, the window can be asked over the network, if it's running.
if [ -n "$text_only" ] || [ -n "$new_window" ]; then
    say "WSL can't run Windows programs here (interoperability is switched off), so this command can only hand files"
    say "to a Limoniter that's already open. For the other options, run limoniter.exe in Windows."
    exit 1
fi
if [ -z "$body" ]; then
    say "There are no files to open."
    exit 2
fi

where=${LIMONITER_WINDOW:-}
if [ -z "$where" ]; then
    note=$(ls -t /mnt/*/Users/*/AppData/Roaming/Limoniter/window.txt 2>/dev/null | head -n 1)
    if [ -n "$note" ]; then
        where=$(awk 'NR == 1 { print $1 ":" $2 }' "$note")
    fi
fi
if [ -z "$where" ]; then
    say "Limoniter doesn't seem to be running in Windows, and WSL can't start it (interoperability is switched off)."
    say "Start Limoniter in Windows, then try again."
    exit 1
fi
port=${where%%:*}
token=${where#*:}

greeting=LIMONITER-OPEN-1
case $body in *@*) greeting=LIMONITER-OPEN-2 ;; esac
[ -n "$wait" ] && greeting=LIMONITER-OPEN-2
request=$greeting
request="$request
$token
"
[ -n "$wait" ] && request="$request@wait
"
request="$request$body
"

hosts=127.0.0.1
gateway=$(ip route show default 2>/dev/null | awk '{ print $3; exit }')
[ -n "$gateway" ] && hosts="$hosts $gateway"
resolver=$(awk '/^nameserver/ { print $2; exit }' /etc/resolv.conf 2>/dev/null)
[ -n "$resolver" ] && hosts="$hosts $resolver"

have() { command -v "$1" >/dev/null 2>&1; }

# Sends the request to one address, and succeeds if the window answered OK (and, with --wait, until it's let go).
send() {
    host=$1
    if have bash; then
        REQUEST=$request WAIT=$wait bash -c '
            exec 3<>"/dev/tcp/$1/$2" || exit 3
            printf "%s" "$REQUEST" >&3
            read -r answer <&3
            [ "$answer" = OK ] || exit 4
            [ -n "$WAIT" ] && read -r more <&3
            exit 0' bash "$host" "$port" 2>/dev/null
    elif have nc; then
        answer=$(printf '%s' "$request" | nc "$host" "$port" 2>/dev/null)
        case $answer in OK*) return 0 ;; esac
        return 4
    elif have curl; then
        answer=$(printf '%s' "$request" | curl -s "telnet://$host:$port" 2>/dev/null)
        case $answer in OK*) return 0 ;; esac
        return 4
    else
        return 5
    fi
}

if ! have bash && ! have nc && ! have curl; then
    say "This command needs bash, nc or curl to reach Limoniter, and none of them is installed here."
    say "Install one (for instance: sudo apt install netcat-openbsd), then try again."
    exit 1
fi
for host in $hosts; do
    send "$host"
    status=$?
    [ "$status" -eq 0 ] && exit 0
done
say "Limoniter didn't answer. Is it running in Windows? If WSL's network isn't shared with Windows, Limoniter has"
say "to be started once from its terminal for this distribution before this command can reach it."
exit 1
