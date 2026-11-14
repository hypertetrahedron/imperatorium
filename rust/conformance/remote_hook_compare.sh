#!/bin/sh
# Run on a host (ssh HOST sh -s < remote_hook_compare.sh): cc_hook.py and the
# binary's `hook`, fed the same events and environment, posting to a capture
# listener on 127.0.0.1:18799 - never the tunnel's 8792. Prints SAME or the
# two bodies.
BIN="$HOME/.claude/imperatorium/bin/imperatorium"
PY="${PY:-$HOME/.claude/imperatorium/cc_hook.py}"
D=$(mktemp -d)
python3 - "$D" <<'EOF' &
import http.server, sys, os
d = sys.argv[1]
n = [0]
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        n[0] += 1
        open(os.path.join(d, "%03d" % n[0]), "wb").write(
            self.headers.get("Content-Type", "").encode() + b"\n" + body)
        self.send_response(200); self.end_headers(); self.wfile.write(b'{"ok":true}')
    def log_message(self, *a): pass
http.server.HTTPServer(("127.0.0.1", 18799), H).serve_forever()
EOF
LISTENER=$!
sleep 1
export IMPERATORIUM_ENDPOINT=http://127.0.0.1:18799/hook
export CLAUDE_CODE_MESSAGING_SOCKET=/tmp/claude-x/inbox.sock CLAUDE_CODE_MESSAGING_TOKEN=tok-123 CLAUDE_PID=4242
unset HC_TERMINAL_ID HC_SESSION CLAUDE_CODE_BRIDGE_SESSION_ID
i=0
for ev in \
 '{"hook_event_name":"SessionStart","session_id":"a1","cwd":"/home/x","source":"startup"}' \
 '{"hook_event_name":"Stop","session_id":"a1","cwd":"/home/x","last_assistant_message":"héllo \"q\"\nline2"}' \
 '{"hook_event_name":"Notification","session_id":"a1","notification_type":"permission_prompt","message":"Claude needs permission"}' \
 '{"hook_event_name":"StopFailure","session_id":"a1","error":"overloaded","error_details":"529"}' \
 '{"hook_event_name":"SessionEnd","session_id":"a1","reason":"other"}'; do
  printf '%s' "$ev" | python3 "$PY"; sleep 0.3
  printf '%s' "$ev" | "$BIN" hook; sleep 0.3
  i=$((i+2))
done
sleep 0.5
kill $LISTENER
same=0; diff=0
for a in $(seq 1 2 $i); do
  b=$((a+1))
  fa=$D/$(printf %03d $a); fb=$D/$(printf %03d $b)
  if python3 -c "import json,sys; a=open(sys.argv[1],'rb').read().split(b'\n',1); b=open(sys.argv[2],'rb').read().split(b'\n',1); sys.exit(0 if a[0]==b[0] and json.loads(a[1])==json.loads(b[1]) else 1)" "$fa" "$fb"; then
    same=$((same+1))
  else
    diff=$((diff+1)); echo "DIFFER:"; cat "$fa"; echo; cat "$fb"; echo
  fi
done
echo "hook bodies: $same same, $diff different (of $((i/2)))"
rm -rf "$D"
