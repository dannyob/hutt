-- hutt-opener.applescript — macOS URL scheme handler for hutt:// URLs
--
-- macOS delivers URLs to registered apps via Apple Events, not argv.
-- This AppleScript receives the "open location" event and delegates
-- to the bundled shell script which does the actual IPC work.

on run
    -- Launched directly (double-click); nothing to do.
end run

on open location theURL
    set myPath to POSIX path of (path to me)
    set shellScript to myPath & "Contents/MacOS/hutt-open.sh"
    do shell script quoted form of shellScript & " " & quoted form of theURL
end open location
