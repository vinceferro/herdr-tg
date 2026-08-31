# `curl -K -` keeps the token off argv; only `-q` keeps it out of ~/.curlrc's reach

The recipe this box uses to send Telegram alarms feeds the token-bearing URL to curl on stdin, so
it never reaches argv or the process table. That is only half the protection.

Without `-q`, curl reads `~/.curlrc` FIRST. One `trace-ascii`, `dump-header` or `output` line in
that file writes the full URL — token included — into a file the caller never chose, at whatever
mode the user's umask gives it. The stdin trick does not help, because the leak happens on curl's
own config path.

There is no `~/.curlrc` on this box today, which is why nothing has leaked. `-q` costs one flag and
closes it for good; `deploy/herdr-tg-watchdog.sh` passes it.

**kickoff's `tg_send_tokenless` (supervisor.sh) has the same gap** and runs on every adopter box on
this machine. Mailed to claude-kickoff 2026-08-31. Its `-o /dev/null` and `2>/dev/null` guard the
body and stderr, not the config file.
