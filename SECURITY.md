# Security

aka edits your shell profiles and writes files your shell runs on every start, so security problems matter here. Thanks for taking the time to report one.

## Reporting a problem

Please don't open a public issue. Use GitHub's private reporting instead:
[report a vulnerability](https://github.com/boltbench/aka/security/advisories/new).

Include what you found, how to reproduce it, and what someone could do with it. I'll reply as soon as I can, usually within a week, and keep you posted until it's fixed.

## What counts

Things like:

- a way for an alias name or command to run code nobody asked for when the init file loads
- aka writing outside its own folder or the shell profiles it says it will touch
- the installers accepting a download that doesn't match its checksum

## Supported versions

Fixes go into the latest release.

## What aka does and doesn't do

aka works entirely on your machine. It never connects to the network, and it collects no usage data or telemetry. `aka suggest` reads your shell history files locally and only reads them. The only downloads are the ones the install scripts make when you run them.
