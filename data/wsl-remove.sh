f="$HOME/.local/bin/limoniter"
if [ -f "$f" ] && grep -q 'put here by Limoniter' "$f"; then rm -f "$f" && echo done; fi
