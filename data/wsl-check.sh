f="$HOME/.local/bin/limoniter"
if [ -f "$f" ] && grep -q 'put here by Limoniter' "$f"; then
    echo mine
    sed -n 's/^# exe: //p' "$f" | head -n 1
elif command -v limoniter >/dev/null 2>&1; then
    echo other
else
    echo none
fi
case ":$PATH:" in *":$HOME/.local/bin:"*) echo onpath ;; esac
