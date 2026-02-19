# hutt-open.ps1 — Windows URL handler for hutt:// URLs
#
# Receives a hutt:// URL and forwards it as a JSON IPC command to the
# running hutt instance via its Unix domain socket.
#
# Requires Windows 10 1803+ (AF_UNIX socket support) and Python 3
# (for the socket send — .NET's Socket with AddressFamily.Unix is
# available on newer Windows but Python is more portable).
#
# Socket location (must match Rust's socket_path()):
#   1. $env:XDG_RUNTIME_DIR\hutt.sock
#   2. $env:TEMP\hutt-$env:USERNAME.sock

param(
    [Parameter(Position=0)]
    [string]$Url
)

if (-not $Url) { exit 0 }

# --- Locate the IPC socket ---------------------------------------------------

$Sock = ""
if ($env:XDG_RUNTIME_DIR -and (Test-Path "$env:XDG_RUNTIME_DIR\hutt.sock")) {
    $Sock = "$env:XDG_RUNTIME_DIR\hutt.sock"
} else {
    # Fallback: match the Unix convention as closely as possible.
    # On Windows the Rust code would need a platform branch; this is the
    # expected default.
    $Sock = "$env:TEMP\hutt-$env:USERNAME.sock"
}

if (-not (Test-Path $Sock)) {
    [System.Windows.Forms.MessageBox]::Show(
        "No running hutt instance found.",
        "Hutt",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Warning
    ) 2>$null
    exit 1
}

# --- Parse the URL and build JSON ---------------------------------------------

$Rest = $Url -replace '^hutt://', ''

# Use Python for reliable JSON construction and AF_UNIX socket send.
$PyScript = @"
import json, socket, sys, urllib.parse

rest = sys.argv[1]
sock_path = sys.argv[2]

if rest.startswith('message/'):
    mid = rest[len('message/'):]
    cmd = json.dumps({'type': 'Open', 'kind': 'Message', 'id': mid})
elif rest.startswith('thread/'):
    tid = rest[len('thread/'):]
    cmd = json.dumps({'type': 'Open', 'kind': 'Thread', 'id': tid})
elif rest.startswith('search/'):
    query = urllib.parse.unquote(rest[len('search/'):])
    cmd = json.dumps({'type': 'Open', 'kind': 'Search', 'query': query})
elif rest.startswith('compose?'):
    params = urllib.parse.parse_qs(rest[len('compose?'):])
    to = params.get('to', [''])[0]
    subject = params.get('subject', [''])[0]
    cmd = json.dumps({'type': 'Open', 'kind': 'Compose', 'to': to, 'subject': subject})
else:
    cmd = json.dumps({'type': 'Open', 'kind': 'Message', 'id': rest})

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock_path)
s.sendall(cmd.encode())
s.close()
"@

python3 -c $PyScript $Rest $Sock
if ($LASTEXITCODE -ne 0) {
    # Try 'python' if 'python3' is not on PATH (common on Windows)
    python -c $PyScript $Rest $Sock
}
