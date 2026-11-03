# Security policy

Earmark is a desktop app that plays audio, decodes music files you choose and saves its settings in
`%APPDATA%\Earmark`. Examples of security problems are a music file that makes Earmark run code or
write files outside its own folder, or a way to tamper with a release download.

## Reporting a vulnerability

Please don't open a public issue. Report it privately through GitHub's
[private vulnerability reporting](https://github.com/josephmurrells/earmark/security/advisories/new)
instead, with the Earmark version or commit, the steps to reproduce it and what an attacker gains.

You should hear back within a week. Once a fix is released, the advisory is published with credit
to you, unless you'd rather stay anonymous.

## Supported versions

Only the latest release and the `master` branch get security fixes.

## Out of scope

Bugs in the crates Earmark is built on, such as Symphonia's decoders, belong to those projects.
Please report them there too; if Earmark needs an update or a workaround, a report here is welcome.
