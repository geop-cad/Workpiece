# Security policy

Workpiece is an early prototype. It reads files from other programs (STEP, IGES, STL, 3MF, and its own `.workpiece` parts) and runs a command line that scripts call, so a malformed file that crashes it, uses unbounded memory or time, or makes it write where it should not is worth reporting.

## Reporting a vulnerability

Please do not open a public issue. Send a report to `tobi.jacob@gmx.net`, with the version or commit, what you did, what happened, and the file that triggers it if there is one.

There is no bug bounty and no promised response time; the project is maintained by a small group of volunteers, and reports are read when the maintainers can.

## Supported versions

There are no releases yet. Only the latest commit of the default branch is looked at.
